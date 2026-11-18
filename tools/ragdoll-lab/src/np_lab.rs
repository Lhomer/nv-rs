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
    /// Downed (M6): the pose's label (none standing under the balance), the loads the walls and
    /// the floor carry (N), whether the legs' push is lifting, the deepest a capsule lies in a wall
    /// (game units), the centre of mass (game units).
    pub label: Option<&'static str>,
    pub wall_load: f32,
    pub floor_load: f32,
    pub lifting: bool,
    pub in_wall: f32,
    pub com: V3,
    /// Substeps the solver's speed guard slowed a body this frame (none expected).
    pub clamped: u32,
    /// Rising (the AI's wish still on), the centre of mass outside the feet (m), the torso's tilt
    /// (degrees).
    pub rising: bool,
    pub over_feet: f32,
    pub tilt: f32,
}

/// A scenario's world and the AI's asks: walls (game units: a point and the normal out), when the
/// AI asks it to stand, its effort (wounds, fatigue: 1 whole), and whether a fall is kept downed
/// (M6) rather than left falling for the game's ragdoll.
#[derive(Clone, Debug)]
pub struct Scene {
    pub walls: Vec<(V3, V3)>,
    pub rise_at: Option<f32>,
    pub effort: f32,
    pub keep_down: bool,
    /// The AI asks again 1 s after a rise gives up (while it is still down).
    pub retry: bool,
}

impl Default for Scene {
    fn default() -> Scene {
        Scene {
            walls: Vec::new(),
            rise_at: None,
            effort: 1.0,
            keep_down: false,
            retry: false,
        }
    }
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
    /// The scene's walls (game units: a point, the normal out) for drawing.
    pub walls: Vec<(V3, V3)>,
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
    run_scene(name, hits, seconds, anim_at, &Scene::default())
}

/// [`run_on`] in `scene`.
pub fn run_scene(
    name: &str,
    hits: &[NpHit],
    seconds: f32,
    anim_at: &dyn Fn(f32) -> Vec<(M3, V3)>,
    scene: &Scene,
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
    np.keep_down = scene.keep_down;
    np.effort = scene.effort;
    np.walls = scene
        .walls
        .iter()
        .map(|(p, n)| ragdoll_core::np_body::Plane {
            p: ragdoll_core::flinch::scale(*p, 1.0 / UNITS),
            n: *n,
        })
        .collect();
    let mut rise_asked = false;
    let mut gave_up: Option<f32> = None;
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
        walls: scene.walls.clone(),
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
        if let Some(at) = scene.rise_at {
            if t >= at && !rise_asked {
                rise_asked = true;
                np.rise = true;
            }
            // Asked again a second after a rise gave up.
            if scene.retry && rise_asked && !np.rise && np.down.is_some() {
                gave_up = gave_up.or(Some(t));
                if gave_up.is_some_and(|g| t - g >= 1.0) {
                    np.rise = true;
                    gave_up = None;
                }
            }
        }
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
            label: np.label().map(|l| l.name()),
            wall_load: np.report.wall_load.iter().sum(),
            floor_load: np.report.floor_load.iter().sum(),
            lifting: np.down.as_ref().is_some_and(|d| d.lifting),
            in_wall: in_wall(&np, &scene.walls),
            com: ragdoll_core::flinch::scale(np.model.centre_of_mass(&np.states).0, UNITS),
            clamped: np.report.clamped,
            rising: np.rise,
            over_feet: np.down.as_ref().map_or(0.0, |d| d.reading.over_feet),
            tilt: np
                .down
                .as_ref()
                .map_or(0.0, |d| d.reading.tilt.to_degrees()),
            nodes,
        });
    }
    out.hits = hits_given;
    out
}

/// The deepest any capsule lies inside a wall (game units; 0 outside them all).
fn in_wall(np: &Np, walls: &[(V3, V3)]) -> f32 {
    let mut most = 0.0f32;
    for (def, s) in np.model.bodies.iter().zip(&np.states) {
        for e in def.ends {
            let p = ragdoll_core::flinch::scale(
                add(s.x, ragdoll_core::active::qrot(s.q, sub(e, def.com))),
                UNITS,
            );
            for (w, n) in walls {
                let d = ragdoll_core::flinch::dot(sub(p, *w), *n) - def.radius * UNITS;
                most = most.max(-d);
            }
        }
    }
    most
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
        "{{\"name\":{:?},\"floor\":{},\"frame\":{},\"walls\":[{}],\"hits\":[{}],\"frames\":[",
        run.name,
        run.floor,
        FRAME,
        run.walls
            .iter()
            .map(|(p, n)| format!(
                "[{:.2},{:.2},{:.2},{:.3},{:.3},{:.3}]",
                p[0], p[1], p[2], n[0], n[1], n[2]
            ))
            .collect::<Vec<_>>()
            .join(","),
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

    // --- M6: downed but active. ------------------------------------------------------------

    /// A downed scene on the standing bind pose: a 4.35 m/s chest hit (past any step), the
    /// fall kept, `walls`, the AI asking to stand at `rise_at`, `effort`, more `hits` after.
    fn downed(
        walls: Vec<(V3, V3)>,
        rise_at: Option<f32>,
        effort: f32,
        more: &[NpHit],
        seconds: f32,
    ) -> NpRun {
        let (_, anim, _) = bind_pose();
        let mut hits = chest_hit(4.35);
        hits.extend_from_slice(more);
        let scene = Scene {
            walls,
            rise_at,
            effort,
            keep_down: true,
            retry: false,
        };
        run_scene("down", &hits, seconds, &|_| anim.clone(), &scene)
    }

    /// The most any node moves (units, degrees) from one frame to the next after `from` s, the
    /// lowest point, the deepest in a wall, the solver's guard's slowings.
    fn down_limits(r: &NpRun, from: f32) -> (f32, f32, f32, f32, u32) {
        let (mut d, mut a) = (0.0f32, 0.0f32);
        for w in r.frames.windows(2).filter(|w| w[0].t >= from) {
            for (x, y) in w[0].nodes.iter().zip(&w[1].nodes) {
                d = d.max(length(sub(y.1, x.1)));
                a = a.max(angle_between(&y.0, &x.0).to_degrees());
            }
        }
        let low = r
            .frames
            .iter()
            .map(|f| f.lowest)
            .fold(f32::INFINITY, f32::min);
        let wall = r.frames.iter().map(|f| f.in_wall).fold(0.0, f32::max);
        // The guard's slowings away from a hit's own frames (its impulse spins the struck body
        // for a substep).
        let near_hit = |t: f32| r.hits.iter().any(|h| (t - h.0).abs() < 0.1);
        let clamped = r
            .frames
            .iter()
            .filter(|f| !near_hit(f.t))
            .map(|f| f.clamped)
            .sum();
        (d, a, low, wall, clamped)
    }

    /// The body still at the end: the centre of mass moving under 2 units over the last 0.5 s.
    fn still_at_end(r: &NpRun) -> f32 {
        let n = r.frames.len();
        let k = (0.5 / FRAME) as usize;
        length(sub(r.frames[n - 1].com, r.frames[n - 1 - k].com))
    }

    /// A big hit with a wall 45 units behind: it falls against the wall and slides down it, and
    /// comes to rest where the physics leaves it, the wall carrying it (at least 200 N on it)
    /// and the label "propped" over the last 2 s; never in the wall more than 1 unit, nothing
    /// under the floor more than 3, the solver's guard never used; still at the end. Without the
    /// wall the same hit ends lying or seated, still, the wall carrying nothing.
    #[test]
    fn np_m6_comes_to_rest_where_the_physics_leaves_it() {
        let wall = vec![([0.0, -45.0, 0.0], [0.0, 1.0, 0.0])];
        let r = downed(wall, None, 1.0, &[], 6.0);
        let late: Vec<&NpFrame> = r.frames.iter().filter(|f| f.t > 4.0).collect();
        let (d, a, low, in_wall, clamped) = down_limits(&r, 1.0);
        let load = late
            .iter()
            .map(|f| f.wall_load)
            .fold(f32::INFINITY, f32::min);
        eprintln!(
            "RESULT np M6 against a wall: labels {:?}, the wall's load at least {load:.0} N, in the wall at most {in_wall:.2} u, lowest {low:.1} u, moves at most {d:.1} u / {a:.1} deg a frame after 1 s, still {:.2} u, guard {clamped}",
            late.iter().map(|f| f.label).collect::<std::collections::BTreeSet<_>>(),
            still_at_end(&r)
        );
        assert!(
            late.iter().all(|f| f.label == Some("propped")),
            "not propped"
        );
        assert!(load > 200.0, "the wall carries {load:.0} N");
        assert!(in_wall < 1.0 && low > -3.0 && clamped == 0);
        assert!(still_at_end(&r) < 2.0);
        let open = downed(Vec::new(), None, 1.0, &[], 6.0);
        let end = open.frames.last().unwrap();
        eprintln!(
            "RESULT np M6 without a wall: ends {:?}, still {:.2} u",
            end.label,
            still_at_end(&open)
        );
        assert!(
            matches!(end.label, Some("lying") | Some("seated")),
            "{:?}",
            end.label
        );
        assert!(still_at_end(&open) < 2.0 && end.wall_load == 0.0);
    }

    /// Asked to stand, a strong, whole actor stands by its balance within 5 s of the ask (on its
    /// back on open floor, and propped on the wall): it sits up, draws its feet in, comes over
    /// them and pushes up, and the balance has it again (no label); its centre of mass ends over
    /// its feet (outside them by at most 0.05 m once the pelvis is over 80% of its height); no node moves more
    /// than 8 units or 25 degrees a frame, nothing under the floor, the guard never used.
    #[test]
    fn np_m6_stands_by_balance() {
        for (name, walls, ask) in [
            ("on its back", Vec::new(), 3.0f32),
            (
                "propped on a wall",
                vec![([0.0, -45.0, 0.0], [0.0, 1.0, 0.0])],
                4.0,
            ),
        ] {
            let r = downed(walls, Some(ask), 1.0, &[], ask + 6.0);
            let stood = r
                .frames
                .iter()
                .find(|f| f.t > ask && f.label.is_none())
                .map(|f| f.t - ask);
            let (d, a, low, _, clamped) = down_limits(&r, 1.5);
            let over = r
                .frames
                .iter()
                .filter(|f| f.t > ask && f.label.is_some() && f.nodes[0].1[2] - r.floor > 54.0)
                .map(|f| f.over_feet)
                .fold(f32::NEG_INFINITY, f32::max);
            let end = r.frames.last().unwrap();
            eprintln!(
                "RESULT np M6 stands ({name}): by balance {:?} s after the ask, the centre at most {over:.2} m outside the feet over 80% up, moves at most {d:.1} u / {a:.1} deg a frame, lowest {low:.1} u, the pelvis {:.0} u up at the end, guard {clamped}",
                stood,
                end.nodes[0].1[2] - r.floor
            );
            assert!(
                stood.is_some_and(|t| t < 5.0),
                "{name}: not standing ({stood:?})"
            );
            assert!(
                end.label.is_none() && end.nodes[0].1[2] - r.floor > 60.0,
                "{name}"
            );
            assert!(
                over < 0.05,
                "{name}: the centre {over:.2} m outside the feet"
            );
            assert!(
                d < 8.0 && a < 25.0,
                "{name}: a frame moved {d:.1} u / {a:.1} deg"
            );
            assert!(
                low > -3.0 && clamped == 0,
                "{name}: lowest {low:.1}, guard {clamped}"
            );
        }
    }

    /// Too weak to push its weight up (effort 0.4: the legs' 600 N under its 795): it tries,
    /// gives up within a second and settles back lying, still, with no half pose held (the pelvis
    /// under 15 units); the wish to stand is off.
    #[test]
    fn np_m6_a_weak_actor_fails_and_settles() {
        let r = downed(Vec::new(), Some(3.0), 0.4, &[], 7.0);
        let end = r.frames.last().unwrap();
        let gave_up = r
            .frames
            .iter()
            .find(|f| f.t > 3.0 && !f.rising)
            .map(|f| f.t - 3.0);
        eprintln!(
            "RESULT np M6 weak: gave up {gave_up:?} s after the ask, ends {:?}, pelvis {:.1} u, still {:.2} u",
            end.label,
            end.nodes[0].1[2] - r.floor,
            still_at_end(&r)
        );
        assert!(gave_up.is_some_and(|t| t < 1.0));
        assert_eq!(end.label, Some("lying"));
        assert!(end.nodes[0].1[2] - r.floor < 15.0 && still_at_end(&r) < 2.0);
    }

    /// A 1.5 m/s chest hit half way up its rise is another impulse on a partly supported body:
    /// whatever follows is physics, and it ends either standing by its balance or settled (lying,
    /// seated or crouched, still); no node moves more than 8 units or 25 degrees a frame except in
    /// the hit's own frame, nothing under the floor, the guard never used.
    #[test]
    fn np_m6_a_hit_during_the_rise() {
        let probe = downed(Vec::new(), Some(3.0), 1.0, &[], 9.0);
        let mid = probe
            .frames
            .iter()
            .find(|f| f.t > 3.0 && f.label == Some("rising"))
            .map_or(5.0, |f| f.t + 0.3);
        let hit = NpHit {
            at: mid,
            body: "Bip01 Spine2",
            offset: [0.0, 8.0, 4.0],
            speed: 1.5,
            dir: [0.0, -1.0, 0.0],
        };
        let r = downed(Vec::new(), Some(3.0), 1.0, &[hit], 11.0);
        let end = r.frames.last().unwrap();
        let after: Vec<&NpFrame> = r.frames.iter().filter(|f| f.t > mid + 0.05).collect();
        let (mut d, mut a) = (0.0f32, 0.0f32);
        for w in after.windows(2) {
            for (x, y) in w[0].nodes.iter().zip(&w[1].nodes) {
                d = d.max(length(sub(y.1, x.1)));
                a = a.max(angle_between(&y.0, &x.0).to_degrees());
            }
        }
        let (_, _, low, _, clamped) = down_limits(&r, 0.0);
        eprintln!(
            "RESULT np M6 hit at {mid:.2} s mid-rise: ends {:?} (pelvis {:.0} u), moves at most {d:.1} u / {a:.1} deg a frame after it, lowest {low:.1}, guard {clamped}",
            end.label,
            end.nodes[0].1[2] - r.floor
        );
        let settled = matches!(end.label, Some("lying") | Some("seated") | Some("crouched"))
            && still_at_end(&r) < 2.0;
        assert!(end.label.is_none() || settled, "{:?}", end.label);
        assert!(d < 8.0 && a < 25.0 && low > -3.0 && clamped == 0);
    }

    /// Whatever pose the fall leaves it in, a strong, whole actor gets up: 18 landings (chest hits
    /// of 3.8, 4.35 and 5 m/s from six directions, the AI asking 2.5 s after and again a second
    /// after a rise gives up): at least 16 stand by their balance within 9 s; in all, nothing
    /// under the floor and the solver's guard never used after the fall's first second (one
    /// still trying is the AI's wish).
    #[test]
    fn np_m6_stands_from_whatever_pose_it_lands_in() {
        let (_, anim, _) = bind_pose();
        let (mut stood, mut n) = (0, 0);
        let mut lines = Vec::new();
        for speed in [3.8f32, 4.35, 5.0] {
            for deg in [-30.0f32, -10.0, 0.0, 15.0, 35.0, 180.0] {
                let a = deg.to_radians();
                let dir = [a.sin(), -a.cos(), 0.0];
                let hits = vec![NpHit {
                    at: 0.2,
                    body: "Bip01 Spine2",
                    offset: [-dir[0] * 8.0, -dir[1] * 8.0, 4.0],
                    speed,
                    dir,
                }];
                let scene = Scene {
                    rise_at: Some(2.7),
                    keep_down: true,
                    retry: true,
                    ..Scene::default()
                };
                let r = run_scene("sweep", &hits, 12.0, &|_| anim.clone(), &scene);
                let up = r
                    .frames
                    .iter()
                    .find(|f| f.t > 2.7 && f.label.is_none())
                    .map(|f| f.t - 2.7);
                n += 1;
                if up.is_some_and(|t| t < 9.0) {
                    stood += 1;
                }
                let (_, _, low, _, _) = down_limits(&r, 0.0);
                // The guard after the fall's first second (a face-first landing whips the light
                // neck past it for a substep at the impact).
                let clamped: u32 = r
                    .frames
                    .iter()
                    .filter(|f| f.t > 1.0)
                    .map(|f| f.clamped)
                    .sum();
                assert!(
                    low > -3.0 && clamped == 0,
                    "{speed} m/s from {deg}: lowest {low:.1}, guard {clamped}"
                );
                lines.push(format!("{speed}/{deg}: {up:.1?}"));
            }
        }
        eprintln!(
            "RESULT np M6 stands from {stood} of {n} landings (s after the ask): {}",
            lines.join(", ")
        );
        assert!(stood >= 16, "{stood} of {n}");
    }
}
