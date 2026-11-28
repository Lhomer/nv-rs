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
            Act::Wait(0.1),
            Act::Hit("Bip01 R Forearm", NINE, From::Front),
            Act::Wait(0.25),
            Act::Hit("Bip01 Spine2", NINE, From::Front),
            Act::Wait(0.25),
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
    /// The uneven spot, its bumps' height and its ground's heights as cast (5 by 5, 0.35 m apart,
    /// against the spot's), once looked for.
    uneven: Option<Option<(V3, f32, [f32; 25])>>,
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
            if run.t >= until && !acting_begins(e, run) {
                run.plan += 1;
                if run.plan >= PLANS.len() {
                    finish(run);
                } else {
                    begin(e, player, run);
                }
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
                progress(
                    run,
                    format!("watched after its last action ({:.0} s)", plan.watch),
                );
                return;
            };
            progress(
                run,
                format!("action {} of {}: {act:?}", i + 1, plan.acts.len()),
            );
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

/// The placed target settled: the scenario's actions begin on it (its reference, never its form
/// id). False, the scenario said as failed, when there is no target to act on (the next is
/// begun).
fn acting_begins(e: &mut Engine, run: &mut Run) -> bool {
    let plan = &PLANS[run.plan];
    let Some((target, _)) = target_ref(e) else {
        run.lines.push(format!(
            "FAIL {}: the range's target could not be found to act on",
            plan.name
        ));
        run.failed.push(plan.name);
        return false;
    };
    run.target = target;
    run.havok_before = crate::new_physics::to_havok(run.target);
    run.frames.clear();
    run.replay = empty(plan.name);
    run.replay.expect = plan.expect.to_string();
    run.t = 0.0;
    run.stage = Stage::Acting(0, 0.0);
    run.standing = 0.0;
    progress(run, "its actions begun".into());
    true
}

/// A scenario with nothing in it yet.
fn empty(name: &str) -> Scenario {
    Scenario {
        name: name.to_string(),
        expect: String::new(),
        standing: 0.0,
        slope: [0.0; 2],
        bumps: [0.0; 2],
        grid: Vec::new(),
        dt: Vec::new(),
        hits: Vec::new(),
        kill: None,
        obstacles: false,
    }
}

/// A reference's form id as the console names it.
fn id(e: &Engine, r: u32) -> String {
    format!("{:08X}", e.mem.u32(r + 0x0c))
}

/// The game's `LookupFormByID`.
const LOOKUP_FORM: u32 = 0x0048_39c0;

/// The phone's range's target as a reference (its form id looked up; looked up afresh each time:
/// the range's reset puts a new one down), and where it first stood; none while the range has no
/// target, or its form id is no reference. `crate::test_range::phone_target` gives the form id:
/// taken for the reference itself, New Physics claimed "actor" FF00131A and the game read its
/// position at FF00134A (Patrick's run on ec4caa3, frame 575).
fn target_ref(e: &mut Engine) -> Option<(u32, V3)> {
    let (form, at, _) = crate::test_range::phone_target()?;
    let r = e.call(LOOKUP_FORM, &args![form]).u32();
    (r != 0 && e.mem.u32(r + 0x0c) == form).then_some((r, at))
}

/// The next scenario begun: the target put back where it first stood (or on the slope).
fn begin(e: &mut Engine, player: u32, run: &mut Run) {
    let plan = &PLANS[run.plan];
    crate::test_range::phone_reset(e, player);
    let Some((target, at)) = target_ref(e) else {
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
    } else if let Some(to) = clear_of_obstacles(e, player, at) {
        // Flat: on clear ground, never by the phone range's obstacles stood behind the target
        // (Patrick's PC run on fcb591c: a railing a metre behind it, every body thrown back came
        // down over it, its feet and calves against it at 10 to 78 kN).
        let t = id(e, target);
        for (axis, v) in ["x", "y", "z"].iter().zip([to[0], to[1], to[2] + 5.0]) {
            crate::test_range::run_line(e, &format!("{t}.setpos {axis} {v:.0}"));
        }
    }
    run.t = 0.0;
    run.stage = Stage::Placing(SETTLE);
    progress(run, "the target placed, settling".into());
}

/// How far toward the player a flat scenario's target stands from the phone range's spot (m): the
/// range's obstacles stand 0.55 to 1 m behind that spot, a fall is under 2 m long.
pub const FLAT_CLEAR: f32 = 3.0;

/// The ground [`FLAT_CLEAR`] toward the player (at `player`'s position) from the target's spot
/// `at` (game units); none if the player stands on the spot.
fn clear_of_obstacles(e: &mut Engine, player: u32, at: V3) -> Option<V3> {
    let u = ragdoll_core::np_body::UNITS;
    let p = crate::new_physics::ref_position(e, player);
    let d = ragdoll_core::flinch::unit([p[0] - at[0], p[1] - at[1], 0.0])?;
    let xy = [at[0] + d[0] * FLAT_CLEAR * u, at[1] + d[1] * FLAT_CLEAR * u];
    let z = ground(e, xy, at[2]).unwrap_or(at[2]);
    Some([xy[0], xy[1], z])
}

/// A body's own ground is cast from this far over it (game units): deeper in the ground than this
/// is not seen as under it.
const GROUND_OVER: f32 = 35.0;

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
/// the bumps' height (half the spread) and the heights cast (row by row, against the spot's).
/// None if the room has none.
fn find_uneven(e: &mut Engine, at: V3) -> Option<(V3, f32, [f32; 25])> {
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
                let mut grid = [0.0f32; 25];
                for (k, z) in hs.iter().flatten().enumerate() {
                    grid[k] = z - h;
                }
                return Some(([p[0], p[1], h], (hi - lo) * 0.5, grid));
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
    // The ground under each body, cast from a little over the body itself ([`GROUND_OVER`]): from
    // over the pelvis' floor, a railing's or a table's top beside the target was taken for the
    // ground under a foot ("21 to 29 units under the ground", always a foot, Patrick's PC run on
    // 5f52c0e).
    let ground_under = drawn
        .iter()
        .map(|b| {
            ground(e, [b.1[0], b.1[1]], b.1[2] + GROUND_OVER - 70.0)
                .filter(|&z| z <= b.1[2] + GROUND_OVER)
                .unwrap_or(floor)
        })
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
        if let Some(Some((_, h, grid))) = run.uneven {
            replay.bumps = [h, 100.0];
            replay.grid = grid.to_vec();
        }
    }
    run.replays.push(replay);
}

/// Where the self-test's files go: beside the game's log (`NV_REPORT_LOG`'s folder), else
/// `NV_REPORT_DIR`, else here. Tests: only the folder a test names ([`tests::OUT`]), none
/// otherwise (a test's progress file was written into the crate's folder, and committed).
fn out_dir() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    {
        tests::OUT.with(|o| o.borrow().clone())
    }
    #[cfg(not(test))]
    Some(
        std::env::var_os("NV_REPORT_LOG")
            .map(std::path::PathBuf::from)
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
            .or_else(|| std::env::var_os("NV_REPORT_DIR").map(std::path::PathBuf::from))
            .unwrap_or_else(|| std::path::PathBuf::from(".")),
    )
}

/// The newest results' file, at a place that never changes (`Launcher.exe
/// --update-and-physics-test` reads it).
const LATEST: &str = "physics-selftest-latest.txt";

/// The results' text: the header (the build, the run's stamp, Delirium, the replay's and the
/// game log's paths), each line, and the last line (`ALL PASS`, `FAILED: ...`, `RUNNING: ...`).
fn results_text(stamp: &str, replay: &std::path::Path, lines: &[String], last: &str) -> String {
    let log = std::env::var_os("NV_REPORT_LOG")
        .map(|p| std::path::PathBuf::from(p).display().to_string())
        .unwrap_or_else(|| "(not known: NV_REPORT_LOG not set)".into());
    let mut text = format!(
        "nv-rs physics self-test\nbuild: {}\nrun: {stamp} UTC\nDelirium: {}\nreplay: {}\ngame \
         log: {log}\n",
        crate::room_record::commit(),
        if delirium_bridge::running() {
            "on"
        } else {
            "off (New Physics alone: what only Delirium's body shows doesn't apply)"
        },
        replay.display()
    );
    for l in lines {
        text += l;
        text += "\n";
    }
    text += last;
    text += "\n";
    text
}

/// What the run has reached, kept apart from [`RUN`] (whose lock a frame holds across the game's
/// calls, and which a fault inside one may leave taken): the scenario, its stage, its seconds,
/// the lines and replays of the scenarios done, and the run's stamp. Read at a stop of the game
/// ([`at_stop`]), so the results say where it was even then.
struct Progress {
    stamp: String,
    scenario: &'static str,
    plan: usize,
    stage: String,
    t: f32,
    lines: Vec<String>,
    failed: Vec<&'static str>,
    replays: Vec<Scenario>,
}

static PROGRESS: Mutex<Option<Progress>> = Mutex::new(None);

/// [`PROGRESS`] brought up to `run` at `stage`, and the results' latest file says it is running
/// (if the game is ended from outside, that is where it was).
fn progress(run: &Run, stage: String) {
    let plan = PLANS.get(run.plan);
    let text = {
        let mut guard = PROGRESS.lock().unwrap_or_else(|p| p.into_inner());
        let p = guard.get_or_insert_with(|| Progress {
            stamp: utc_stamp(),
            scenario: "",
            plan: 0,
            stage: String::new(),
            t: 0.0,
            lines: Vec::new(),
            failed: Vec::new(),
            replays: Vec::new(),
        });
        p.t = run.t;
        // The same stage again (an action waiting, frame by frame): nothing new to say.
        if p.plan == run.plan && p.stage == stage && !p.scenario.is_empty() {
            return;
        }
        p.scenario = plan.map_or("the end", |p| p.name);
        p.plan = run.plan;
        p.stage = stage;
        p.lines = run.lines.clone();
        p.failed = run.failed.clone();
        p.replays = run.replays.clone();
        let replay = out_dir()
            .unwrap_or_default()
            .join(format!("physics-selftest-{}-replay.txt", p.stamp));
        results_text(
            &p.stamp,
            &replay,
            &p.lines,
            &format!(
                "RUNNING: {} ({} of {}), {}",
                p.scenario,
                p.plan + 1,
                PLANS.len(),
                p.stage
            ),
        )
    };
    if let Some(dir) = out_dir() {
        let _ = std::fs::write(dir.join(LATEST), text.as_bytes());
    }
}

/// The game stopped (or ended) while the self-test ran: its results written all the same, with
/// the scenarios done, the one it was in and its stage, and the stop's line, ending `FAILED`
/// (Patrick's run on ec4caa3: a crash at frame 575 left no results). Called by the game's stop
/// (`crate::player_move`); nothing when the self-test is off or ended.
pub fn at_stop(line: &str) {
    if on() {
        stopped(line);
    }
}

/// [`at_stop`]'s results, the self-test on.
fn stopped(line: &str) {
    let guard = match PROGRESS.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::Poisoned(g)) => g.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return,
    };
    let Some(p) = guard.as_ref() else {
        return;
    };
    if p.scenario == "the end" {
        return;
    }
    let Some(dir) = out_dir() else {
        return;
    };
    let replay = dir.join(format!("physics-selftest-{}-replay.txt", p.stamp));
    let mut lines = p.lines.clone();
    lines.push(format!(
        "FAIL {}: the game stopped {:.1} s into it ({}): {line}",
        p.scenario, p.t, p.stage
    ));
    let mut failed = p.failed.clone();
    failed.push(p.scenario);
    let text = results_text(
        &p.stamp,
        &replay,
        &lines,
        &format!(
            "FAILED: {} (the game stopped in {})",
            failed.join(", "),
            p.scenario
        ),
    );
    let _ = std::fs::write(&replay, crate::physics_replay::write(&p.replays));
    let _ = std::fs::write(
        dir.join(format!("physics-selftest-{}.txt", p.stamp)),
        text.as_bytes(),
    );
    let _ = std::fs::write(dir.join(LATEST), text.as_bytes());
    eprintln!(
        "physics self-test: the game stopped in {} ({}): results written to {}",
        p.scenario,
        p.stage,
        dir.join(LATEST).display()
    );
}

/// The results and the replay written, the game ended.
fn finish(run: &mut Run) {
    let stamp = PROGRESS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map_or_else(utc_stamp, |p| p.stamp.clone());
    let dir = out_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let results = dir.join(format!("physics-selftest-{stamp}.txt"));
    let replay = dir.join(format!("physics-selftest-{stamp}-replay.txt"));
    let replay_text = crate::physics_replay::write(&run.replays);
    let last = if run.failed.is_empty() {
        "ALL PASS".to_string()
    } else {
        format!("FAILED: {}", run.failed.join(", "))
    };
    let text = results_text(&stamp, &replay, &run.lines, &last);
    // Ended: a stop from here on (the game's own end) says nothing more.
    if let Some(p) = PROGRESS.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        p.scenario = "the end";
    }
    let latest = dir.join(LATEST);
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
pub(super) mod tests {
    use super::*;

    thread_local! {
        /// The folder this test's self-test files go to (none: not written).
        pub static OUT: std::cell::RefCell<Option<std::path::PathBuf>> =
            const { std::cell::RefCell::new(None) };
    }

    /// The range keeps its target as a form id (FF00131A in Patrick's run); the self-test acts on
    /// the reference it names, looked up with the game's `LookupFormByID`, never on the form id
    /// itself (taken for a reference, New Physics claimed FF00131A and the game read FF00134A:
    /// the crash at frame 575 on ec4caa3). With no reference for it, the scenario fails and is
    /// passed, nothing acted on.
    #[test]
    fn the_selftest_acts_on_the_targets_reference_not_its_form_id() {
        const FORM: u32 = 0xff00_131a;
        const REFERENCE: u32 = 0x0300_0000;
        let mut e = Engine::new();
        e.mem.map(REFERENCE, 0x1000);
        e.mem.set_u32(REFERENCE + 0x0c, FORM);
        e.register_double(LOOKUP_FORM, |_, a| {
            if a[0] == FORM { REFERENCE } else { 0 }.into_ret()
        });
        crate::test_range::set_phone_target_for_tests(Some((FORM, [1.0, 2.0, 3.0])));
        let mut run = Run {
            plan: 0,
            stage: Stage::Placing(0.0),
            t: 3.0,
            total: 3.0,
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
        };
        assert!(acting_begins(&mut e, &mut run));
        assert_eq!(run.target, REFERENCE, "{:08X}", run.target);
        assert!(matches!(run.stage, Stage::Acting(0, _)));
        assert_eq!(run.replay.name, PLANS[0].name);
        assert_eq!(target_ref(&mut e), Some((REFERENCE, [1.0, 2.0, 3.0])));
        // A form id that names no reference: the scenario fails, nothing acted on.
        crate::test_range::set_phone_target_for_tests(Some((0xff00_2000, [0.0; 3])));
        let mut run2 = Run {
            target: 0,
            lines: Vec::new(),
            failed: Vec::new(),
            ..run
        };
        assert!(!acting_begins(&mut e, &mut run2));
        assert_eq!(run2.target, 0);
        assert_eq!(run2.failed, [PLANS[0].name]);
        crate::test_range::set_phone_target_for_tests(None);
        assert_eq!(target_ref(&mut e), None);
    }

    /// The game stopping mid-run still leaves results: the scenarios done, the one it was in with
    /// its stage and seconds, the stop's line, and `FAILED` last (what the launcher reads as a
    /// failed run, exit 1); while it runs, the latest file says where it is.
    #[test]
    fn a_stop_mid_run_still_writes_the_results() {
        let dir = std::env::temp_dir().join(format!("nv-selftest-stop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        OUT.with(|o| *o.borrow_mut() = Some(dir.clone()));
        let run = Run {
            plan: 1,
            stage: Stage::Acting(0, 0.0),
            t: 0.4,
            total: 9.0,
            target: 0,
            frames: Vec::new(),
            replay: empty(PLANS[1].name),
            havok_before: 0,
            standing: 0.0,
            lines: vec![format!(
                "PASS {}: no pose jumps (PC and CI): ...",
                PLANS[0].name
            )],
            failed: Vec::new(),
            replays: vec![empty(PLANS[0].name)],
            slope: None,
            uneven: None,
            struck: 0.0,
        };
        progress(&run, "action 1 of 1: a hit".into());
        let running = std::fs::read_to_string(dir.join(LATEST)).unwrap();
        assert!(
            running.trim_end().ends_with(&format!(
                "RUNNING: {} (2 of {}), action 1 of 1: a hit",
                PLANS[1].name,
                PLANS.len()
            )),
            "{running}"
        );
        let stop = "Game's main stopped in frame 575: read of unmapped memory at ff00134a (in \
                    009445b0 < 0093e860)";
        stopped(stop);
        let text = std::fs::read_to_string(dir.join(LATEST)).unwrap();
        assert!(
            text.starts_with("nv-rs physics self-test\nbuild: "),
            "{text}"
        );
        assert!(text.contains(&format!("PASS {}: no pose jumps", PLANS[0].name)));
        assert!(
            text.contains(&format!(
                "FAIL {}: the game stopped 0.4 s into it (action 1 of 1: a hit): {stop}",
                PLANS[1].name
            )),
            "{text}"
        );
        let last = text.lines().last().unwrap();
        assert_eq!(
            last,
            format!(
                "FAILED: {} (the game stopped in {})",
                PLANS[1].name, PLANS[1].name
            )
        );
        // The stamped copy and the replay of the scenarios done are there too.
        let files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            files.iter().any(|f| f.ends_with("-replay.txt")),
            "{files:?}"
        );
        assert_eq!(files.len(), 3, "{files:?}");
        OUT.with(|o| *o.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
