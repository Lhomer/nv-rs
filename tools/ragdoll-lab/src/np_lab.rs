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
    let (model, anim, floor) = bind_pose();
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
        let nodes = np.frame(&anim, floor, FRAME);
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
            nodes,
        });
    }
    out.hits = hits_given;
    out
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
        s.push_str(&format!(
            "{{\"t\":{:.4},\"caption\":{:?},\"caps\":[",
            f.t,
            caption(f)
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
    /// top 6, 14 and 25 degrees off its animation (within 20%), the head with it (0.8 to 1.2 times),
    /// and the torso's bend over the pelvis is back under 2 degrees within 0.7 s; nothing under the
    /// floor, the joints together, no node jumping (at most 8 units or 25 degrees a frame).
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
                rc.back <= 0.7,
                "{v} m/s: back under 2 degrees after {:.2} s",
                rc.back
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
}
