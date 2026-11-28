//! The hit reactions' checks, the same in the game's self-test on the player's PC
//! (`crate::physics_selftest`, with Delirium) and in the CI's replay over the stand-in body
//! (`crate::delirium_stand_in`): each frame an actor's body is watched ([`Frame`]), then
//! [`judge`] says each check's PASS or FAIL with its figures. A check marked [`Where::Pc`] is about
//! what Delirium's own body does (it stays up, it rises): over the stand-in it only shows the
//! integration's sequencing lets it, so it counts only on the player's PC.

use ragdoll_core::flinch::{length, sub, M3, V3};
use ragdoll_core::np_body::UNITS;

/// One frame of an actor watched.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    /// Seconds since the scenario began, and this frame's seconds.
    pub t: f32,
    pub dt: f32,
    /// The bodies drawn (each body's node's world, game units); empty: New Physics has let go.
    pub drawn: Vec<(M3, V3)>,
    /// The floor under the actor (game units).
    pub floor: f32,
    /// Who has the body (`crate::new_physics::Watch::owner`; `animation` once let go).
    pub owner: &'static str,
    /// Waiting for, or handed to, the game's ragdoll.
    pub havok: bool,
    /// The open hit's most against a wall (N) and on the floor (body weights).
    pub wall: f32,
    pub floor_load: f32,
    /// The game has the actor dead.
    pub dead: bool,
}

/// What a scenario expects of the body.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Expect {
    /// A light hit: it stays on its feet.
    pub stays_standing: bool,
    /// It goes down and must stand again.
    pub rises: bool,
    /// It dies: its corpse Delirium's.
    pub dies: bool,
    /// Its standing pelvis height over the floor (game units).
    pub standing: f32,
}

/// Where a check counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Where {
    /// Everywhere: the integration's own (frames, hand-overs, limits, the ground).
    Both,
    /// Only with real Delirium (the player's PC): what its body does.
    Pc,
}

/// One check's verdict.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    pub name: &'static str,
    pub pass: bool,
    pub detail: String,
    pub counts: Where,
}

/// The limits (Patrick's 17:47 asks).
pub const STANDING_LOW: f32 = 0.85;
pub const UPRIGHT_LEAN: f32 = 15.0;
pub const UPRIGHT_PELVIS: f32 = 0.9;
/// The settle and the get-up's budget after it goes down (s): 3 s to settle, up to 5 s to rise,
/// and a second for the fall itself.
pub const RISE_BUDGET: f32 = 9.0;
/// At the end of the rise (a second either side of upright) no bone moves more in a frame (game
/// units).
pub const SNAP_MOST: f32 = 10.0;
/// No bone ever moves faster than this (m/s), nor more than [`JUMP_MOST`] in a frame.
pub const SPEED_MOST: f32 = 9.0;
pub const JUMP_MOST: f32 = 25.0;
/// The most against a wall (N) and on the floor (body weights) a body may carry.
pub const WALL_MOST: f32 = 20000.0;
pub const FLOOR_MOST: f32 = 25.0;
/// Down or dead, no joint further out of the skeleton's limits (degrees).
pub const JOINT_MOST: f32 = 5.0;
/// Lying this long (s) before the joints are judged.
pub const LIMITS_AFTER: f32 = 0.25;
/// No body's node further under the floor (game units).
pub const UNDER_MOST: f32 = 5.0;
/// Stood again, the game's animation has it back within this (s).
pub const BACK_WITHIN: f32 = 6.0;

/// The pelvis over the floor (game units) and the torso's lean from upright (degrees) of a pose.
pub fn stance(drawn: &[(M3, V3)], floor: f32) -> Option<(f32, f32)> {
    let m = crate::new_physics::model();
    let pelvis = drawn.get(m.root)?.1;
    let head = m.body("Bip01 Head").and_then(|b| drawn.get(b))?.1;
    let up = sub(head, pelvis);
    let l = length(up);
    let lean = if l > 1e-3 {
        (up[2] / l).clamp(-1.0, 1.0).acos().to_degrees()
    } else {
        0.0
    };
    Some((pelvis[2] - floor, lean))
}

/// The most any body's node moved between two frames (game units).
pub fn moved_most(a: &[(M3, V3)], b: &[(M3, V3)]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| length(sub(x.1, y.1)))
        .fold(0.0, f32::max)
}

/// Whether `owner` is a body lying down or dead (where the joint limits must hold).
fn lying(owner: &str) -> bool {
    owner.ends_with(" down") || owner.ends_with(" dead")
}

/// Each check's verdict for `frames` against `expect`, with `to_havok` the times the actor went
/// to the game's ragdoll and `corpse_held` whether the game kept the corpse's own ragdoll on the
/// pose drawn (`None`: not asked, the CI's stand-in).
pub fn judge(
    frames: &[Frame],
    expect: &Expect,
    to_havok: usize,
    corpse_held: Option<bool>,
) -> Vec<Check> {
    let mut out = Vec::new();
    let mut check = |name, pass, detail: String, counts| {
        out.push(Check {
            name,
            pass,
            detail,
            counts,
        })
    };
    let st = expect.standing.max(1.0);
    let poses: Vec<(f32, f32, f32)> = frames
        .iter()
        .filter_map(|f| stance(&f.drawn, f.floor).map(|(p, l)| (f.t, p, l)))
        .collect();
    let upright = |p: f32, l: f32| p >= UPRIGHT_PELVIS * st && l < UPRIGHT_LEAN;
    let released = frames.iter().position(|f| f.drawn.is_empty() && f.t > 0.0);
    // Each frame's most moving body, its speed.
    let mut jump = (0.0f32, 0.0f32);
    let mut fastest = (0.0f32, 0.0f32);
    for w in frames.windows(2) {
        if w[0].drawn.is_empty() || w[1].drawn.is_empty() || w[1].dt <= 0.0 {
            continue;
        }
        let d = moved_most(&w[0].drawn, &w[1].drawn);
        if d > jump.0 {
            jump = (d, w[1].t);
        }
        let v = d / UNITS / w[1].dt;
        if v > fastest.0 {
            fastest = (v, w[1].t);
        }
    }
    if expect.stays_standing {
        let low = poses.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        check(
            "light hit stays standing",
            low >= STANDING_LOW * st,
            format!(
                "the pelvis at least {low:.0} u over the floor of {st:.0} standing (at least {:.0})",
                STANDING_LOW * st
            ),
            Where::Pc,
        );
        let last = poses.last().copied();
        let ok = released.is_some() || last.is_some_and(|(_, p, l)| upright(p, l));
        check(
            "recovers upright",
            ok,
            match (released, last) {
                (Some(i), _) => format!("back to the animation at {:.1} s", frames[i].t),
                (None, Some((_, p, l))) => {
                    format!("at the end the pelvis {p:.0} u, the torso {l:.0} degrees")
                }
                (None, None) => "never drawn".into(),
            },
            Where::Pc,
        );
    }
    if expect.rises {
        let down_at = poses
            .iter()
            .find(|p| p.1 < 0.55 * st || p.2 > 60.0)
            .map(|p| p.0);
        let up_at = down_at.and_then(|d| {
            poses
                .iter()
                .find(|p| p.0 > d && upright(p.1, p.2))
                .map(|p| p.0)
        });
        check(
            "rise reaches upright in time",
            match (down_at, up_at) {
                (Some(d), Some(u)) => u - d <= RISE_BUDGET,
                _ => false,
            },
            match (down_at, up_at) {
                (None, _) => "never went down".into(),
                (Some(d), None) => format!("down at {d:.1} s, never upright"),
                (Some(d), Some(u)) => {
                    format!(
                        "down at {d:.1} s, upright at {u:.1} s ({:.1} s, at most {RISE_BUDGET:.0})",
                        u - d
                    )
                }
            },
            Where::Pc,
        );
        // No snap: a second either side of the first upright frame.
        let snap = up_at.map(|u| {
            frames
                .windows(2)
                .filter(|w| {
                    (w[1].t - u).abs() <= 1.0 && !w[0].drawn.is_empty() && !w[1].drawn.is_empty()
                })
                .map(|w| moved_most(&w[0].drawn, &w[1].drawn))
                .fold(0.0f32, f32::max)
        });
        check(
            "the rise ends with no snap",
            snap.is_some_and(|s| s <= SNAP_MOST),
            match snap {
                Some(s) => format!("a bone at most {s:.1} u in a frame at the end of the rise (at most {SNAP_MOST:.0})"),
                None => "the rise never ended upright".into(),
            },
            Where::Both,
        );
    }
    if expect.rises || expect.stays_standing {
        let back = released.map(|i| frames[i].t);
        let up = poses.iter().rev().find(|p| upright(p.1, p.2)).map(|p| p.0);
        check(
            "the AI has it again",
            match (back, up) {
                (Some(b), Some(u)) => b - u <= BACK_WITHIN || b <= u,
                (Some(_), None) => true,
                _ => false,
            },
            match back {
                Some(b) => format!("New Physics let it go to its animation at {b:.1} s"),
                None => "New Physics never let it go".into(),
            },
            Where::Both,
        );
    }
    if expect.dies {
        let delirium = frames.iter().any(|f| f.owner == "delirium dead");
        check(
            "the corpse is Delirium's",
            delirium && to_havok == 0 && corpse_held != Some(false),
            format!(
                "a Delirium corpse {}, the game's ragdoll held on its pose {}",
                if delirium { "yes" } else { "no" },
                match corpse_held {
                    Some(true) => "yes (lootable, draggable)",
                    Some(false) => "no",
                    None => "(not asked here)",
                }
            ),
            Where::Both,
        );
    }
    check(
        "no pose jumps",
        jump.0 <= JUMP_MOST,
        format!(
            "a bone at most {:.1} u in a frame (at {:.1} s; at most {JUMP_MOST:.0})",
            jump.0, jump.1
        ),
        Where::Both,
    );
    let wall = frames.iter().map(|f| f.wall).fold(0.0f32, f32::max);
    let floor_load = frames.iter().map(|f| f.floor_load).fold(0.0f32, f32::max);
    check(
        "no launch",
        fastest.0 <= SPEED_MOST && wall <= WALL_MOST && floor_load <= FLOOR_MOST,
        format!(
            "the fastest bone {:.1} m/s (at {:.1} s; at most {SPEED_MOST:.0}), a wall at most {wall:.0} N \
             (at most {WALL_MOST:.0}), the floor at most {floor_load:.1} body weights (at most {FLOOR_MOST:.0})",
            fastest.0, fastest.1
        ),
        Where::Both,
    );
    // Judged once it has lain a moment ([`LIMITS_AFTER`]): a rise cut back down starts from New
    // Physics' rise pose (its thighs past the data's limits, as its squat has them), brought in
    // over a few of Delirium's steps.
    let mut lying_for = 0.0f32;
    let mut joint = 0.0f32;
    for f in frames {
        if lying(f.owner) && !f.drawn.is_empty() {
            lying_for += f.dt;
            if lying_for >= LIMITS_AFTER {
                joint = joint.max(crate::delirium_np::into_limits(&f.drawn, 0).1.to_degrees());
            }
        } else {
            lying_for = 0.0;
        }
    }
    check(
        "no joint past its limit while down",
        joint <= JOINT_MOST,
        format!(
            "a joint at most {joint:.1} degrees out while down or dead (at most {JOINT_MOST:.0})"
        ),
        Where::Both,
    );
    let under = frames
        .iter()
        .filter(|f| !f.drawn.is_empty())
        .map(|f| {
            f.drawn
                .iter()
                .map(|b| f.floor - b.1[2])
                .fold(f32::NEG_INFINITY, f32::max)
        })
        .fold(f32::NEG_INFINITY, f32::max);
    check(
        "never under the floor",
        under <= UNDER_MOST,
        format!(
            "the lowest node {:.1} u under the floor at most (at most {UNDER_MOST:.0})",
            under.max(0.0)
        ),
        Where::Both,
    );
    let havok_frames = frames.iter().filter(|f| f.havok).count();
    check(
        "never the game's ragdoll",
        to_havok == 0 && havok_frames == 0,
        format!(
            "{to_havok} hand-overs to the game's ragdoll, {havok_frames} frames waiting for one"
        ),
        Where::Both,
    );
    out
}

/// The results' lines for a scenario: `PASS|FAIL <scenario>: <check> (<where>): <figures>`, and
/// whether every check that counts here passed (`on_pc`: the player's PC, where every check counts).
pub fn lines(scenario: &str, checks: &[Check], on_pc: bool) -> (Vec<String>, bool) {
    let mut all = true;
    let lines = checks
        .iter()
        .map(|c| {
            let counts = on_pc || c.counts == Where::Both;
            if counts && !c.pass {
                all = false;
            }
            format!(
                "{} {scenario}: {} ({}): {}",
                match (c.pass, counts) {
                    (true, _) => "PASS",
                    (false, true) => "FAIL",
                    (false, false) => "INFO",
                },
                c.name,
                match c.counts {
                    Where::Both => "PC and CI",
                    Where::Pc => "PC only",
                },
                c.detail
            )
        })
        .collect();
    (lines, all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing_pose(at: V3) -> Vec<(M3, V3)> {
        let sk = ragdoll_core::np_skeleton::Skeleton::fnv();
        let bind = sk.bind_worlds();
        crate::new_physics::model()
            .bodies
            .iter()
            .map(|b| {
                let w = &bind[sk.node(&b.node).unwrap()];
                (w.r, ragdoll_core::flinch::add(w.t, at))
            })
            .collect()
    }

    fn frames_of(poses: &[Vec<(M3, V3)>], owner: &'static str) -> Vec<Frame> {
        poses
            .iter()
            .enumerate()
            .map(|(i, p)| Frame {
                t: i as f32 / 60.0,
                dt: 1.0 / 60.0,
                drawn: p.clone(),
                floor: 0.0,
                owner,
                ..Default::default()
            })
            .collect()
    }

    /// A body that stands still passes the light hit's checks; one that sinks to half its height
    /// fails "stays standing"; a jump of 40 units in a frame fails "no pose jumps" and "no
    /// launch"; a hand-over to the game's ragdoll fails "never the game's ragdoll".
    #[test]
    fn the_checks_tell_a_good_run_from_a_bad_one() {
        let pose = standing_pose([0.0; 3]);
        let st = stance(&pose, 0.0).unwrap().0;
        let expect = Expect {
            stays_standing: true,
            standing: st,
            ..Default::default()
        };
        let mut good = frames_of(&vec![pose.clone(); 120], "delirium hit");
        good.push(Frame {
            t: 2.1,
            dt: 1.0 / 60.0,
            ..Default::default()
        });
        let checks = judge(&good, &expect, 0, None);
        assert!(checks.iter().all(|c| c.pass), "{checks:?}");
        let sunk: Vec<_> = (0..120)
            .map(|i| standing_pose([0.0, 0.0, -(i as f32) * 0.3]))
            .collect();
        let checks = judge(&frames_of(&sunk, "delirium hit"), &expect, 0, None);
        assert!(
            !checks
                .iter()
                .find(|c| c.name == "light hit stays standing")
                .unwrap()
                .pass
        );
        let mut jumped = vec![pose.clone(); 60];
        jumped.push(standing_pose([40.0, 0.0, 0.0]));
        let checks = judge(&frames_of(&jumped, "delirium hit"), &expect, 1, None);
        let find = |n: &str| checks.iter().find(|c| c.name == n).unwrap().pass;
        assert!(!find("no pose jumps") && !find("no launch") && !find("never the game's ragdoll"));
        let (lines, all) = lines("light-standing", &checks, false);
        assert!(
            !all && lines
                .iter()
                .any(|l| l.starts_with("FAIL light-standing: no pose jumps"))
        );
        assert!(lines
            .iter()
            .any(|l| l.starts_with("INFO") || l.starts_with("PASS")));
    }
}
