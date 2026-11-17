//! New Physics in the lab: FNV's own ragdoll (`ragdoll_core::np`, the game's code) standing in its
//! skeleton's bind pose (the animation), hit, frame by frame: the scenarios, their metrics and
//! the frames for a video (`examples/np_frames.rs`, rendered by `scripts/np_video.py`).

use ragdoll_core::flinch::{add, length, sub, M3, V3};
use ragdoll_core::np::Np;
use ragdoll_core::np_body::{lowest_point, Model, UNITS};
use ragdoll_core::np_skeleton::{angle_between, Skeleton};

/// A frame's step (s).
pub const FRAME: f32 = 1.0 / 60.0;

/// A hit in a scenario: when (s), the struck body's node, the point's offset from that body's node
/// (game units, world), the body's speed it gives (m/s: the impulse is that times the body's
/// mass) and its direction.
#[derive(Clone, Debug)]
pub struct NpHit {
    pub at: f32,
    pub body: &'static str,
    pub offset: V3,
    pub speed: f32,
    pub dir: V3,
}

/// What a scenario drew, frame by frame.
#[derive(Clone, Debug, Default)]
pub struct NpFrame {
    pub t: f32,
    /// Each body's node's world (rotation, game units).
    pub nodes: Vec<(M3, V3)>,
    /// The spine's top (Spine2) and the head turned off the animation against the pelvis (the
    /// root body: the torso's own bend, the recoil), and the spine's top against the world
    /// (degrees).
    pub spine_top: f32,
    pub head: f32,
    pub spine_world: f32,
    /// The lowest point of the body over the floor (game units).
    pub lowest: f32,
    /// The largest joint gap (game units) and the most a joint lay past its limit (degrees).
    pub gap: f32,
    pub past_limit: f32,
    /// The balance: its phase (0 standing, 1 stepping, 2 recovering, 3 falling), its catching
    /// steps so far, how far the capture point is outside the support (m), the actor's moves so
    /// far (game units, the animation moved by them).
    pub phase: u8,
    pub steps: u32,
    pub outside: f32,
    pub moved: V3,
}

/// A scenario's run.
#[derive(Clone, Debug, Default)]
pub struct NpRun {
    pub name: String,
    /// The hits as given (time, world point in game units, direction).
    pub hits: Vec<(f32, V3, V3)>,
    pub frames: Vec<NpFrame>,
    /// The model's capsules (radius, ends in the body's frame, game units) for drawing.
    pub capsules: Vec<(f32, [V3; 2])>,
    pub floor: f32,
}

/// FNV's ragdoll's bind pose: each body's node's world (game units), the model, the floor under
/// it (game units).
pub fn bind_pose() -> (Model, Vec<(M3, V3)>, f32) {
    let sk = Skeleton::fnv();
    let model = Model::from_skeleton(&sk);
    let w = sk.bind_worlds();
    let nodes: Vec<(M3, V3)> = model
        .bodies
        .iter()
        .map(|b| {
            let x = w[sk.node(&b.node).expect("a body's node")];
            (x.r, x.t)
        })
        .collect();
    let states = model.states_from_nodes(&nodes);
    let floor = lowest_point(&model, &states) * UNITS;
    (model, nodes, floor)
}

/// Runs `hits` on the standing body for `seconds`.
pub fn run(name: &str, hits: &[NpHit], seconds: f32) -> NpRun {
    let (_, anim, _) = bind_pose();
    run_on(name, hits, seconds, &|_| anim.clone())
}

/// Runs `hits` on the body over the animation `anim_at(t)` (each body's node's world, game units)
/// for `seconds`; the actor (the animation) moved by what the controller hands it each frame.
pub fn run_on(
    name: &str,
    hits: &[NpHit],
    seconds: f32,
    anim_at: &dyn Fn(f32) -> Vec<(M3, V3)>,
) -> NpRun {
    let (model, _, floor) = bind_pose();
    let mut moved: V3 = [0.0; 3];
    let moved_anim = |t: f32, moved: V3| -> Vec<(M3, V3)> {
        anim_at(t)
            .into_iter()
            .map(|(r, x)| (r, add(x, moved)))
            .collect()
    };
    let anim = moved_anim(0.0, moved);
    let capsules = model
        .bodies
        .iter()
        .map(|b| {
            let k = UNITS;
            let c = b.com;
            let _ = c;
            (
                b.radius * k,
                [
                    ragdoll_core::flinch::scale(b.ends[0], k),
                    ragdoll_core::flinch::scale(b.ends[1], k),
                ],
            )
        })
        .collect();
    let spine = model.body("Bip01 Spine2").expect("a spine top");
    let head = model.body("Bip01 Head").expect("a head");
    let mut np = Np::new(model, &anim, 1.0);
    // Already moving with its animation (a walk): its speed from the start.
    let next = moved_anim(FRAME, moved);
    let r0 = np.model.root;
    np.set_anim_velocity(ragdoll_core::flinch::scale(
        sub(next[r0].1, anim[r0].1),
        1.0 / FRAME,
    ));
    let mut out = NpRun {
        name: name.to_string(),
        capsules,
        floor,
        ..NpRun::default()
    };
    let mut pending: Vec<&NpHit> = hits.iter().collect();
    let mut hits_given: Vec<(f32, V3, V3)> = Vec::new();
    let frames = (seconds / FRAME).round() as usize;
    for i in 0..frames {
        let t = i as f32 * FRAME;
        pending.retain(|h| {
            if h.at > t + 1e-4 {
                return true;
            }
            let b = np.model.body(h.body).expect("a struck body");
            let node = np.model.node_worlds(&np.states)[b].1;
            let mass = np.model.mass;
            let j = ragdoll_core::flinch::scale(h.dir, h.speed * mass);
            np.hit(b, add(node, h.offset), j);
            hits_given.push((t, add(node, h.offset), h.dir));
            false
        });
        let anim = moved_anim(t + FRAME, moved);
        let nodes = np.frame(&anim, floor, FRAME);
        moved = add(moved, np.take_move());
        let lowest = lowest_point(&np.model, &np.states) * UNITS - floor;
        let root = np.model.root;
        let against_root = |b: usize| {
            let now = ragdoll_core::flinch::mul(
                &ragdoll_core::flinch::transpose(&nodes[root].0),
                &nodes[b].0,
            );
            let was = ragdoll_core::flinch::mul(
                &ragdoll_core::flinch::transpose(&anim[root].0),
                &anim[b].0,
            );
            angle_between(&now, &was).to_degrees()
        };
        out.frames.push(NpFrame {
            t: t + FRAME,
            spine_top: against_root(spine),
            head: against_root(head),
            spine_world: angle_between(&nodes[spine].0, &anim[spine].0).to_degrees(),
            lowest,
            gap: np.report.gap_most * UNITS,
            past_limit: np.report.limit_most.to_degrees(),
            phase: match np.phase() {
                ragdoll_core::np_balance::Phase::Standing => 0,
                ragdoll_core::np_balance::Phase::Stepping => 1,
                ragdoll_core::np_balance::Phase::Recovering => 2,
                ragdoll_core::np_balance::Phase::Falling => 3,
            },
            steps: np.balance.taken.iter().filter(|s| !s.2).count() as u32,
            outside: np.balance.outside,
            moved,
            nodes,
        });
    }
    out.hits = hits_given;
    out
}

/// A walk (FNV's skeleton walked by a procedural gait, standing in for the game's walk cycle):
/// forward (+Y) at `speed` game units a second; each leg swinging `swing` radians either way
/// about its hip, the knee bending on the forward swing, the pelvis bobbing with the stance leg; the
/// stride matched to the speed so the stance foot stands still. Each body's node's world at `t`.
pub fn walk(speed: f32, swing: f32) -> impl Fn(f32) -> Vec<(M3, V3)> {
    let sk = Skeleton::fnv();
    let model = Model::from_skeleton(&sk);
    let bind = sk.bind_worlds();
    let body_nodes: Vec<usize> = model
        .bodies
        .iter()
        .map(|b| sk.node(&b.node).expect("a body's node"))
        .collect();
    let thigh = [
        model.body("Bip01 L Thigh").unwrap(),
        model.body("Bip01 R Thigh").unwrap(),
    ];
    let calf = [
        model.body("Bip01 L Calf").unwrap(),
        model.body("Bip01 R Calf").unwrap(),
    ];
    let foot = [
        model.body("Bip01 L Foot").unwrap(),
        model.body("Bip01 R Foot").unwrap(),
    ];
    let leg = ragdoll_core::flinch::length(sub(
        bind[body_nodes[foot[0]]].t,
        bind[body_nodes[thigh[0]]].t,
    ));
    // A step's length (one leg's swing back to front) and the cycle's rate (rad/s).
    let stride = 2.0 * leg * swing.sin();
    let omega = std::f32::consts::PI * speed / stride.max(1.0);
    move |t: f32| {
        let mut w: Vec<(M3, V3)> = body_nodes.iter().map(|&n| (bind[n].r, bind[n].t)).collect();
        let mut drop = 0.0f32;
        for side in 0..2 {
            let phase = omega * t + if side == 0 { 0.0 } else { std::f32::consts::PI };
            let a = swing * phase.sin();
            // Forward swing (the angle rising): the knee bends to clear the floor.
            let bend = if phase.cos() > 0.0 {
                0.7 * phase.cos() * swing * 2.0
            } else {
                0.0
            };
            if phase.cos() <= 0.0 {
                drop = drop.max(leg * (1.0 - a.cos()));
            }
            let hip = w[thigh[side]].1;
            // About the hips' axis (+X): a positive angle swings the leg forward (+Y).
            let turn = ragdoll_core::flinch::rotation([a, 0.0, 0.0]);
            for &b in &[thigh[side], calf[side], foot[side]] {
                let (r, x) = w[b];
                w[b] = (
                    ragdoll_core::flinch::mul(&turn, &r),
                    add(hip, ragdoll_core::flinch::apply_m(&turn, sub(x, hip))),
                );
            }
            let knee = w[calf[side]].1;
            // The shin back from the thigh.
            let kturn = ragdoll_core::flinch::rotation([-bend, 0.0, 0.0]);
            for &b in &[calf[side], foot[side]] {
                let (r, x) = w[b];
                w[b] = (
                    ragdoll_core::flinch::mul(&kturn, &r),
                    add(knee, ragdoll_core::flinch::apply_m(&kturn, sub(x, knee))),
                );
            }
        }
        let along = [0.0, speed * t, -drop];
        w.iter().map(|(r, x)| (*r, add(*x, along))).collect()
    }
}

/// A chest hit from the front at `speed` m/s (the body's speed it gives), at 0.2 s.
pub fn chest_hit(speed: f32) -> Vec<NpHit> {
    vec![NpHit {
        at: 0.2,
        body: "Bip01 Spine2",
        offset: [0.0, 8.0, 4.0],
        speed,
        dir: [0.0, -1.0, 0.0],
    }]
}

/// A recoil's figures: the spine top's peak (degrees), when it is back under 2 degrees for good
/// (s after the hit), the largest frame-to-frame move of a node after the hit (units, degrees),
/// the lowest point (units over the floor), the largest gap.
#[derive(Clone, Copy, Debug)]
pub struct Recoil {
    /// The spine top's peak against the world (the design's figure) and against the pelvis.
    pub world_peak: f32,
    pub peak: f32,
    pub head_peak: f32,
    pub back: f32,
    /// When the bend first falls back under 2 degrees after its peak (s after the hit).
    pub first_back: f32,
    pub step_most: (f32, f32),
    pub lowest: f32,
    pub gap: f32,
}

pub fn recoil(run: &NpRun, hit_at: f32) -> Recoil {
    let after: Vec<&NpFrame> = run.frames.iter().filter(|f| f.t > hit_at).collect();
    let peak = after.iter().map(|f| f.spine_top).fold(0.0, f32::max);
    let world_peak = after.iter().map(|f| f.spine_world).fold(0.0, f32::max);
    let head_peak = after.iter().map(|f| f.head).fold(0.0, f32::max);
    let back = after
        .iter()
        .rposition(|f| f.spine_top >= 2.0)
        .map_or(0.0, |i| after[(i + 1).min(after.len() - 1)].t - hit_at);
    let peak_at = after
        .iter()
        .position(|f| f.spine_top >= peak - 1e-4)
        .unwrap_or(0);
    let first_back = after[peak_at..]
        .iter()
        .find(|f| f.spine_top < 2.0)
        .map_or(f32::INFINITY, |f| f.t - hit_at);
    let mut step_most = (0.0f32, 0.0f32);
    for w in run.frames.windows(2) {
        if w[1].t <= hit_at + FRAME {
            continue;
        }
        for (a, b) in w[0].nodes.iter().zip(&w[1].nodes) {
            step_most.0 = step_most.0.max(length(sub(b.1, a.1)));
            step_most.1 = step_most.1.max(angle_between(&b.0, &a.0).to_degrees());
        }
    }
    Recoil {
        world_peak,
        peak,
        head_peak,
        back,
        first_back,
        step_most,
        lowest: run
            .frames
            .iter()
            .map(|f| f.lowest)
            .fold(f32::INFINITY, f32::min),
        gap: run.frames.iter().map(|f| f.gap).fold(0.0, f32::max),
    }
}

/// The frames as JSON for the video (`scripts/np_video.py`): the capsules' world ends and radii
/// each frame (game units), the floor, a caption line per frame.
pub fn frames_json(run: &NpRun, caption: impl Fn(&NpFrame) -> String) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{{\"name\":{:?},\"floor\":{},\"frame\":{},\"hits\":[{}],\"frames\":[",
        run.name,
        run.floor,
        FRAME,
        run.hits
            .iter()
            .map(|(t, p, d)| format!(
                "[{t:.4},{:.2},{:.2},{:.2},{:.3},{:.3},{:.3}]",
                p[0], p[1], p[2], d[0], d[1], d[2]
            ))
            .collect::<Vec<_>>()
            .join(",")
    ));
    for (i, f) in run.frames.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let focus = f.nodes.first().map_or([0.0; 3], |n| n.1);
        s.push_str(&format!(
            "{{\"t\":{:.4},\"caption\":{:?},\"focus\":[{:.2},{:.2},{:.2}],\"caps\":[",
            f.t,
            caption(f),
            focus[0],
            focus[1],
            focus[2]
        ));
        for (k, ((r, t), (radius, ends))) in f.nodes.iter().zip(&run.capsules).enumerate() {
            if k > 0 {
                s.push(',');
            }
            let p = |e: V3| add(*t, ragdoll_core::flinch::apply_m(r, e));
            let (a, b) = (p(ends[0]), p(ends[1]));
            s.push_str(&format!(
                "[{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}]",
                a[0], a[1], a[2], b[0], b[1], b[2], radius
            ));
        }
        s.push_str("]}");
    }
    s.push_str("]}");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Standing without a hit, FNV's ragdoll holds its animation: the spine's top and head within
    /// a degree over 2 s, nothing under the floor, the joints together.
    #[test]
    fn np_stands_on_its_animation() {
        let r = run("stand", &[], 2.0);
        let most = r
            .frames
            .iter()
            .map(|f| f.spine_top.max(f.head))
            .fold(0.0, f32::max);
        assert!(most < 1.0, "{most:.2} degrees off");
        let rc = recoil(&r, 0.0);
        assert!(rc.lowest > -1.0 && rc.gap < 0.1, "{rc:?}");
    }

    /// The recoil (Patrick's design's targets): a chest hit at 0.3, 0.8 and 2 m/s turns the spine's
    /// top 6, 14 and 25 degrees off its animation (within 20%; the 2 m/s one 0.8 to 1.2 times as its
    /// catching step takes some), the head with it (0.8 to 1.2 times), the torso's bend over the
    /// pelvis back under 2 degrees within 0.7 s; nothing under the floor, the joints together, no
    /// node jumping (at most 8 units or 25 degrees a frame).
    #[test]
    fn np_recoils_and_recovers() {
        for (v, want) in [(0.3, 6.0), (0.8, 14.0), (2.0, 25.0)] {
            let r = run("recoil", &chest_hit(v), 1.6);
            let rc = recoil(&r, 0.2);
            eprintln!("RESULT np recoil {v} m/s: {rc:?}");
            assert!(
                rc.world_peak >= 0.8 * want && rc.world_peak <= 1.2 * want,
                "{v} m/s: peak {:.1} degrees, want {want}",
                rc.world_peak
            );
            let head = rc.head_peak / rc.world_peak;
            assert!(
                (0.8..=1.2).contains(&head),
                "{v} m/s: the head {head:.2} times the spine"
            );
            assert!(
                rc.first_back <= 0.7,
                "{v} m/s: back under 2 degrees after {:.2} s",
                rc.first_back
            );
            assert!(
                rc.lowest > -1.0,
                "{v} m/s: {:.1} under the floor",
                rc.lowest
            );
            assert!(rc.gap < 0.3, "{v} m/s: a joint {:.2} apart", rc.gap);
            assert!(
                rc.step_most.0 < 8.0 && rc.step_most.1 < 25.0,
                "{v} m/s: {:?}",
                rc.step_most
            );
        }
    }

    fn steps(r: &NpRun) -> u32 {
        r.frames.last().map_or(0, |f| f.steps)
    }

    /// A step only when the capture point leaves the feet: a 0.3 m/s chest hit takes none; 0.8 m/s
    /// takes one or two catching steps back, never falls, stands again by 2.5 s (the actor moved
    /// back with its feet, at least 8 units), the spine back under 3 degrees; a 2 m/s one at most
    /// three, caught by 3 s; nothing under the floor.
    #[test]
    fn np_steps_only_past_its_feet() {
        let small = run("small", &chest_hit(0.3), 2.0);
        assert_eq!(steps(&small), 0, "a step for a 0.3 m/s hit");
        assert!(small.frames.iter().all(|f| f.phase == 0));
        for (v, most) in [(0.8f32, 2u32), (2.0, 3)] {
            let r = run("push", &chest_hit(v), 3.5);
            let n = steps(&r);
            let last = r.frames.last().unwrap();
            eprintln!(
                "RESULT np push {v} m/s: {n} catching steps, moved {:?}, last phase {}, spine {:.1}",
                last.moved, last.phase, last.spine_world
            );
            assert!((1..=most).contains(&n), "{v} m/s: {n} catching steps");
            assert!(r.frames.iter().all(|f| f.phase != 3), "{v} m/s: it fell");
            assert!(last.moved[1] < -8.0, "{v} m/s: moved {:?}", last.moved);
            let late: Vec<&NpFrame> = r.frames.iter().filter(|f| f.t > 3.0).collect();
            assert!(
                late.iter().all(|f| f.phase == 0),
                "{v} m/s: not standing by 3 s"
            );
            assert!(
                last.spine_world < 3.0,
                "{v} m/s: spine {:.1} at the end",
                last.spine_world
            );
            assert!(
                r.frames.iter().all(|f| f.lowest > -1.5),
                "{v} m/s: under the floor"
            );
        }
    }

    /// Pushed from its left (toward +X) at 0.8 m/s it steps out with its right foot first and
    /// never crosses its legs (the left foot's node stays left of the right's).
    #[test]
    fn np_a_side_push_steps_out_without_crossing() {
        let hit = vec![NpHit {
            at: 0.2,
            body: "Bip01 Spine2",
            offset: [-8.0, 0.0, 4.0],
            speed: 0.8,
            dir: [1.0, 0.0, 0.0],
        }];
        let r = run("side", &hit, 3.0);
        let (model, _, _) = bind_pose();
        let (lf, rf) = (
            model.body("Bip01 L Foot").unwrap(),
            model.body("Bip01 R Foot").unwrap(),
        );
        let first = r.frames.iter().find(|f| f.steps >= 1);
        assert!(first.is_some(), "no step");
        let moved_right = r.frames.last().unwrap().moved[0];
        eprintln!("RESULT np side push: moved {moved_right:.1} across");
        assert!(moved_right > 5.0, "moved {moved_right:.1} across");
        for f in &r.frames {
            assert!(
                f.nodes[lf].1[0] < f.nodes[rf].1[0] - 4.0,
                "the legs crossed at {:.2} s",
                f.t
            );
        }
    }

    /// Hit while walking (FNV's skeleton on a 101 units a second gait) at 0.8 and 2 m/s: it recoils
    /// and the walk carries on (the knock moves its path), never falls; within 1.5 s of the hit the
    /// drawn body is back on its walk (every node within 12 units of the walk's, its path moved).
    #[test]
    fn np_a_walking_hit_resumes_the_walk() {
        let w = walk(101.0, 0.35);
        for v in [0.8f32, 2.0] {
            let hits = vec![NpHit {
                at: 1.0,
                body: "Bip01 Spine2",
                offset: [0.0, 8.0, 4.0],
                speed: v,
                dir: [0.0, -1.0, 0.0],
            }];
            let r = run_on("walk", &hits, 3.0, &w);
            let off = |f: &NpFrame| {
                f.nodes
                    .iter()
                    .zip(w(f.t))
                    .map(|(n, a)| length(sub(n.1, add(a.1, f.moved))))
                    .fold(0.0f32, f32::max)
            };
            let peak = r
                .frames
                .iter()
                .filter(|f| f.t > 1.0)
                .map(|f| f.spine_world)
                .fold(0.0, f32::max);
            let late = r
                .frames
                .iter()
                .filter(|f| f.t > 2.5)
                .map(off)
                .fold(0.0, f32::max);
            eprintln!("RESULT np walking hit {v} m/s: recoil {peak:.1} degrees, off its walk at most {late:.1} units after 1.5 s, moved {:?}", r.frames.last().unwrap().moved);
            assert!(r.frames.iter().all(|f| f.phase != 3), "{v} m/s: it fell");
            assert!(peak > 5.0, "{v} m/s: no recoil ({peak:.1})");
            assert!(
                late < 12.0,
                "{v} m/s: {late:.1} units off its walk after 1.5 s"
            );
        }
    }
}
