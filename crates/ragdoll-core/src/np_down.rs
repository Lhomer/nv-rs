//! New Physics downed but still fighting (M6): what the body rests on, what the pose is called,
//! and what the AI can do from it. Nothing here poses the body. The controller ([`crate::np`])
//! keeps one goal after a fall: hold the body on whatever is touching it and work back toward
//! balance; wherever the physics leaves the body is its pose.
//!
//! - **Reading** ([`read`]): the body's contacts and the loads they carry (the floor, the walls),
//!   the pelvis's height, the torso's tilt, the centre of mass against the feet and against
//!   everything touching.
//! - **Labels** ([`classify`], [`Labeler`]): lying, seated, propped, crouched, rising, standing,
//!   read from that, held [`LABEL_HOLD`] before they change. For the AI's choice and the log only.
//! - **The AI's reads** ([`reads`]): can it aim from here (the weapon arm not carrying the body,
//!   the target within the shoulder's reach of the torso's facing), can it push up (feet or knees
//!   under it, the legs' strength left over the body's weight), can it crawl.
//! - **Rising** (the controller's goals, `np::Np`): the feet drawn under the hips (leg IK onto a
//!   point under them), the torso leaning over the feet, and once the centre of mass is over the
//!   feet the legs' push ([`RISE_FORCE`] times strength and effort) lifting the pelvis. A weak or
//!   wounded actor whose push is under its weight does not make it and settles back.

use crate::active::{qconj, qrot, Q};
use crate::flinch::{add, dot, length, scale, sub, V3};
use crate::np_balance::{hull, outside};
use crate::np_body::{Model, State, StepReport, GRAVITY};

/// How long a new label must hold before it is the label (s).
pub const LABEL_HOLD: f32 = 0.25;
/// The highest a lying body's pelvis lies over its ground (m: 30 game units).
pub const LYING_MOST: f32 = 30.0 / crate::np_body::UNITS;
/// A foot's sole counts in the support while it carries or lies within this (m) of the floor
/// (3 cm left out a foot 3.7 cm up for a moment as the legs pushed a body up: the centre read
/// 0.07 m outside the other foot alone, the lab's M6 stand).
pub const SOLE_NEAR: f32 = 0.05;
/// The most an arm's contacts may carry and the arm still be free to aim (N).
pub const ARM_FREE: f32 = 40.0;
/// The most a target may lie off the torso's facing and the shoulder still bring the weapon onto
/// it (degrees, flat).
pub const AIM_REACH: f32 = 100.0;
/// The legs' push when rising at full strength and effort (N): the root's support's up cap
/// standing (2000 N) less what a squat costs.
pub const RISE_FORCE: f32 = 1500.0;
/// A rise whose pelvis has got no higher for this long gives up (s).
pub const RISE_STALL: f32 = 1.5;
/// The pelvis at this share of its standing height (with the feet carrying it and the torso up)
/// is standing again.
pub const STOOD: f32 = 0.88;
/// Each joint's spring share and gravity hold on the ground (by the child body's node): the legs
/// and the torso hold the support they lie on, the head kept up, the arms loose.
pub const DOWN_GAINS: [(&str, f32, f32); 6] = [
    ("Thigh", 0.15, 0.0),
    ("Calf", 0.15, 0.0),
    ("Foot", 0.3, 0.0),
    ("Spine", 0.35, 0.3),
    ("Neck1", 1.0, 1.0),
    ("Head", 1.0, 1.0),
];
/// The arms' share on the ground.
pub const DOWN_ARMS: f32 = 0.4;

/// A pose's name, read from the simulated body (never a target).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    Lying,
    Seated,
    Propped,
    Crouched,
    Rising,
    Standing,
}

impl Label {
    pub fn name(self) -> &'static str {
        match self {
            Label::Lying => "lying",
            Label::Seated => "seated",
            Label::Propped => "propped",
            Label::Crouched => "crouched",
            Label::Rising => "rising",
            Label::Standing => "standing",
        }
    }
}

/// A label kept from flickering: a new reading must hold [`LABEL_HOLD`] to become the label.
#[derive(Clone, Copy, Debug)]
pub struct Labeler {
    pub label: Label,
    candidate: Label,
    held: f32,
}

impl Labeler {
    pub fn new(label: Label) -> Labeler {
        Labeler {
            label,
            candidate: label,
            held: 0.0,
        }
    }

    /// The reading `raw` this frame; answers the new label when it changed.
    pub fn update(&mut self, raw: Label, dt: f32) -> Option<Label> {
        if raw == self.label {
            self.candidate = raw;
            self.held = 0.0;
            return None;
        }
        // Up off the ground from lying is taken at once (held, a quick rise read "lying" with
        // the pelvis 40 units up).
        if self.label == Label::Lying
            && matches!(raw, Label::Crouched | Label::Rising | Label::Standing)
        {
            self.label = raw;
            self.candidate = raw;
            self.held = 0.0;
            return Some(raw);
        }
        if raw != self.candidate {
            self.candidate = raw;
            self.held = 0.0;
        }
        self.held += dt;
        if self.held >= LABEL_HOLD {
            self.label = raw;
            self.held = 0.0;
            return Some(raw);
        }
        None
    }
}

/// The bodies the reading looks at, and each one's up and forward in its own frame (the bind
/// pose's world up and forward).
#[derive(Clone, Debug)]
pub struct Bodies {
    pub root: usize,
    pub torso: usize,
    pub head: usize,
    pub spines: Vec<usize>,
    pub thighs: [usize; 2],
    pub calves: [usize; 2],
    pub feet: [usize; 2],
    /// Each arm's bodies (upper arm, forearm, hand): left, right (the weapon arm).
    pub arms: [[usize; 3]; 2],
    /// Each body's world up (+z) and forward (the way the feet point in the bind pose) in its own
    /// frame.
    pub up: Vec<V3>,
    pub forward: Vec<V3>,
    /// The root's centre's height over the floor standing (m).
    pub stand_height: f32,
    pub weight: f32,
    /// The living body's reach ([`reach_pose`]): each joint's relative rotation in a deep squat
    /// with the spine curled. A downed body's limits take it in besides the data's and the
    /// animation's (the data's are a dead ragdoll's: 45 degrees at the hip, no one rises inside
    /// them).
    pub reach: Vec<Q>,
}

impl Bodies {
    pub fn new(model: &Model) -> Bodies {
        let b = |n: &str| model.body(n).unwrap_or(model.root);
        let (_, nodes) = model.bind();
        let states = model.states_from_nodes(&nodes);
        let floor = crate::np_body::lowest_point(model, &states);
        // The skeleton faces the way its toes point from its heels.
        let foot = b("Bip01 L Foot");
        let toe = {
            let def = &model.bodies[foot];
            let e: Vec<V3> = def
                .ends
                .iter()
                .map(|e| add(states[foot].x, qrot(states[foot].q, sub(*e, def.com))))
                .collect();
            let d = sub(e[0], e[1]);
            crate::flinch::unit([d[0], d[1], 0.0]).unwrap_or([0.0, 1.0, 0.0])
        };
        let in_frame = |q: Q, v: V3| qrot(qconj(q), v);
        Bodies {
            root: model.root,
            torso: b("Bip01 Spine2"),
            head: b("Bip01 Head"),
            spines: ["Bip01 Spine", "Bip01 Spine1", "Bip01 Spine2"]
                .iter()
                .filter_map(|n| model.body(n))
                .collect(),
            thighs: [b("Bip01 L Thigh"), b("Bip01 R Thigh")],
            calves: [b("Bip01 L Calf"), b("Bip01 R Calf")],
            feet: [b("Bip01 L Foot"), b("Bip01 R Foot")],
            arms: [
                [
                    b("Bip01 L UpperArm"),
                    b("Bip01 L Forearm"),
                    b("Bip01 L Hand"),
                ],
                [
                    b("Bip01 R UpperArm"),
                    b("Bip01 R Forearm"),
                    b("Bip01 R Hand"),
                ],
            ],
            up: states
                .iter()
                .map(|s| in_frame(s.q, [0.0, 0.0, 1.0]))
                .collect(),
            forward: states.iter().map(|s| in_frame(s.q, toe)).collect(),
            stand_height: states[model.root].x[2] - floor,
            weight: model.mass * GRAVITY,
            reach: reach_pose(model, &nodes, toe),
        }
    }

    /// Body `b`'s world up and forward now.
    pub fn up_of(&self, states: &[State], b: usize) -> V3 {
        qrot(states[b].q, self.up[b])
    }
    pub fn forward_of(&self, states: &[State], b: usize) -> V3 {
        qrot(states[b].q, self.forward[b])
    }
}

/// How far the reach pose folds each leg (the ankle's goal from the hip, shares of the leg's
/// length: forward, down) and curls the spine (rad a joint).
pub const REACH_FOLD: (f32, f32) = (0.25, 0.15);
pub const REACH_CURL: f32 = 0.45;

/// The living body's reach from the bind pose `nodes` (game units) facing `forward`: each leg
/// folded by the legs' IK ([`crate::flinch::leg_ik`], the knee forward) to an ankle
/// [`REACH_FOLD`] from its hip, each spine joint curled forward [`REACH_CURL`]; answers each
/// joint's relative rotation.
pub fn reach_pose(model: &Model, nodes: &[(crate::flinch::M3, V3)], forward: V3) -> Vec<Q> {
    let b = |n: &str| model.body(n);
    let mut posed = nodes.to_vec();
    for side in ["L", "R"] {
        let (Some(thigh), Some(calf), Some(foot)) = (
            b(&format!("Bip01 {side} Thigh")),
            b(&format!("Bip01 {side} Calf")),
            b(&format!("Bip01 {side} Foot")),
        ) else {
            continue;
        };
        let hip = nodes[thigh].1;
        let leg = length(sub(nodes[foot].1, hip));
        let goal = add(
            hip,
            add(
                scale(forward, REACH_FOLD.0 * leg),
                [0.0, 0.0, -REACH_FOLD.1 * leg],
            ),
        );
        let Some((r1, m, _)) =
            crate::flinch::leg_ik(hip, nodes[calf].1, nodes[foot].1, hip, goal, forward)
        else {
            continue;
        };
        let mr1 = crate::flinch::mul(&m, &r1);
        posed[thigh].0 = crate::flinch::mul(&r1, &nodes[thigh].0);
        posed[calf].0 = crate::flinch::mul(&mr1, &nodes[calf].0);
        // The foot turned with its shin but half way back to level: the ankle bent as a squat
        // bends it.
        let level = nodes[foot].0;
        let with = crate::flinch::mul(&mr1, &nodes[foot].0);
        let q = crate::active::qslerp(
            crate::active::qfrom_m(&level),
            crate::active::qfrom_m(&with),
            0.5,
        );
        posed[foot].0 = crate::active::m_from_q(q);
    }
    let mut rel = crate::np::relative_of(model, &posed);
    // The spine curled forward: about the body's side axis (up turned toward forward).
    let side = crate::flinch::cross([0.0, 0.0, 1.0], forward);
    let states = model.states_from_nodes(nodes);
    for (j, def) in model.joints.iter().enumerate() {
        if !model.bodies[def.child].node.starts_with("Bip01 Spine") {
            continue;
        }
        let axis = qrot(qconj(states[def.parent].q), side);
        rel[j] = crate::active::qnorm(crate::active::qmul(
            crate::active::qexp(scale(axis, -REACH_CURL)),
            rel[j],
        ));
    }
    rel
}

/// What the body rests on and how it stands, read from the simulation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Reading {
    /// The root's centre over the floor, and that standing (m).
    pub pelvis_height: f32,
    pub stand_height: f32,
    /// The torso's tilt off the vertical (rad).
    pub tilt: f32,
    /// The loads (N): the feet's on the floor, the seat's (the root and thighs), the hands' and
    /// forearms', the torso's and head's against walls, the weapon arm's (anything).
    pub feet: f32,
    pub seat: f32,
    pub hands: f32,
    pub back_wall: f32,
    pub weapon_arm: f32,
    /// Everything's on the floor and the walls (N).
    pub total: f32,
    pub weight: f32,
    /// The centre of mass and its velocity (m, m/s).
    pub com: V3,
    pub com_v: V3,
    /// The centre of mass outside the feet's soles (m; negative inside; large with no foot down)
    /// and outside everything touching the floor.
    pub over_feet: f32,
    pub over_support: f32,
    /// The torso's facing (flat, unit).
    pub facing: V3,
    /// The weight the legs' push carries (N; rising), counted on the feet.
    pub pushed: f32,
}

/// Reads the body `states` after the step that answered `report`, the floor at `floor` (m).
pub fn read(
    model: &Model,
    b: &Bodies,
    states: &[State],
    report: &StepReport,
    floor: f32,
) -> Reading {
    let load = |list: &[f32], i: usize| list.get(i).copied().unwrap_or(0.0);
    let floor_at = |i: usize| load(&report.floor_load, i);
    let wall_at = |i: usize| load(&report.wall_load, i);
    let feet = b.feet.iter().map(|&i| floor_at(i)).sum::<f32>();
    let seat = floor_at(b.root) + b.thighs.iter().map(|&i| floor_at(i)).sum::<f32>();
    let hands = b
        .arms
        .iter()
        .flat_map(|a| a[1..].iter())
        .map(|&i| floor_at(i) + wall_at(i))
        .sum::<f32>();
    let back_wall = b
        .spines
        .iter()
        .chain([&b.root, &b.head])
        .map(|&i| wall_at(i))
        .sum::<f32>();
    let weapon_arm = b.arms[1].iter().map(|&i| floor_at(i) + wall_at(i)).sum();
    let total = report.floor_load.iter().sum::<f32>() + report.wall_load.iter().sum::<f32>();
    let (com, com_v) = model.centre_of_mass(states);
    let up = b.up_of(states, b.torso);
    let tilt = up[2].clamp(-1.0, 1.0).acos();
    let f = b.forward_of(states, b.torso);
    let facing = crate::flinch::unit([f[0], f[1], 0.0])
        .or_else(|| crate::flinch::unit([up[0], up[1], 0.0]))
        .unwrap_or([0.0, 1.0, 0.0]);
    // The feet's soles, and every contact on the floor.
    let mut soles: Vec<[f32; 2]> = Vec::new();
    for &foot in &b.feet {
        if floor_at(foot) > 1.0 || lowest(model, states, foot) < floor + SOLE_NEAR {
            let def = &model.bodies[foot];
            for e in def.ends {
                let p = add(states[foot].x, qrot(states[foot].q, sub(e, def.com)));
                for d in [[-1.0f32, 0.0], [1.0, 0.0], [0.0, -1.0], [0.0, 1.0]] {
                    soles.push([p[0] + d[0] * def.radius, p[1] + d[1] * def.radius]);
                }
            }
        }
    }
    let feet_hull = hull(&soles);
    let mut touching: Vec<[f32; 2]> = report.contacts.iter().map(|c| [c.1[0], c.1[1]]).collect();
    touching.extend(&soles);
    let all = hull(&touching);
    let c2 = [com[0], com[1]];
    let out = |h: &Vec<[f32; 2]>| {
        if h.is_empty() {
            10.0
        } else {
            outside(h, c2)
        }
    };
    Reading {
        pelvis_height: states[b.root].x[2] - floor,
        stand_height: b.stand_height,
        tilt,
        feet,
        seat,
        hands,
        back_wall,
        weapon_arm,
        total,
        weight: b.weight,
        com,
        com_v,
        over_feet: out(&feet_hull),
        over_support: out(&all),
        facing,
        pushed: 0.0,
    }
}

/// The lowest point of body `i` (m).
fn lowest(model: &Model, states: &[State], i: usize) -> f32 {
    let def = &model.bodies[i];
    def.ends
        .iter()
        .map(|e| add(states[i].x, qrot(states[i].q, sub(*e, def.com)))[2] - def.radius)
        .fold(f32::INFINITY, f32::min)
}

/// The pose's name from a reading; `rising`: the controller is working to stand.
pub fn classify(r: &Reading, rising: bool) -> Label {
    let h = r.pelvis_height / r.stand_height.max(0.1);
    let tilt = r.tilt.to_degrees();
    let w = r.weight.max(1.0);
    let feet = r.feet + r.pushed;
    if h >= STOOD && tilt < 35.0 && feet > 0.5 * w {
        return Label::Standing;
    }
    if rising && h > 0.4 && r.com_v[2] > 0.05 {
        return Label::Rising;
    }
    if feet > 0.4 * w && h > 0.3 {
        return Label::Crouched;
    }
    if r.back_wall > 60.0 && tilt < 70.0 {
        return Label::Propped;
    }
    if r.seat > 0.25 * w && tilt < 55.0 {
        return Label::Seated;
    }
    // Up on its hands and knees (or propped on them) is no lying: the 14:09 clip's "lying" with
    // the pelvis 66 units up.
    if r.pelvis_height >= LYING_MOST {
        return Label::Crouched;
    }
    Label::Lying
}

/// What the AI reads from the pose.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reads {
    /// The weapon arm carries none of the body.
    pub arm_free: bool,
    /// The target off the torso's facing (degrees, flat; 0 with no target).
    pub aim_off: f32,
    /// Free and within reach (line of sight is the host's).
    pub can_aim: bool,
    /// The legs' push left over the body's weight (N), and whether a push up can start (something
    /// under it to push from, the push over the weight).
    pub push_margin: f32,
    pub can_push_up: bool,
    /// Something touching that can move it (a limb on the floor, the effort to drag).
    pub can_crawl: bool,
}

/// The AI's reads from reading `r`: the target's direction from the actor (`target`, flat or
/// not), the legs' push now (`push`, N).
pub fn reads(r: &Reading, target: Option<V3>, push: f32) -> Reads {
    let arm_free = r.weapon_arm < ARM_FREE;
    let aim_off = target
        .and_then(|t| crate::flinch::unit([t[0], t[1], 0.0]))
        .map_or(0.0, |t| {
            dot(t, r.facing).clamp(-1.0, 1.0).acos().to_degrees()
        });
    let push_margin = push - r.weight;
    let under = r.feet > 1.0 || r.seat > 1.0 || r.hands > 1.0;
    Reads {
        arm_free,
        aim_off,
        can_aim: arm_free && target.is_some() && aim_off <= AIM_REACH,
        push_margin,
        can_push_up: under && push_margin > 0.0,
        can_crawl: r.total > 0.2 * r.weight && push > 0.3 * r.weight,
    }
}

/// A rotation vector turning `from` onto `to` (unit vectors; world).
pub fn turn_between(from: V3, to: V3) -> V3 {
    let c = crate::flinch::cross(from, to);
    let s = length(c);
    let cos = dot(from, to).clamp(-1.0, 1.0);
    if s < 1e-6 {
        return [0.0; 3];
    }
    scale(c, s.atan2(cos) / s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new label holds only after [`LABEL_HOLD`]; a flicker back resets it.
    #[test]
    fn a_label_changes_only_once_it_holds() {
        let mut l = Labeler::new(Label::Standing);
        let dt = 1.0 / 60.0;
        for _ in 0..10 {
            assert_eq!(l.update(Label::Seated, dt), None);
        }
        assert_eq!(l.update(Label::Standing, dt), None);
        let mut changed = None;
        for _ in 0..20 {
            changed = changed.or(l.update(Label::Seated, dt));
        }
        assert_eq!(changed, Some(Label::Seated));
        assert_eq!(l.label, Label::Seated);
    }

    /// Standing in the bind pose reads standing: the feet carry it, the torso upright, the
    /// pelvis at its height.
    #[test]
    fn the_bind_pose_reads_standing() {
        let m = Model::fnv();
        let b = Bodies::new(&m);
        let (_, nodes) = m.bind();
        let states = m.states_from_nodes(&nodes);
        let floor = crate::np_body::lowest_point(&m, &states);
        let mut report = StepReport {
            floor_load: vec![0.0; m.bodies.len()],
            wall_load: vec![0.0; m.bodies.len()],
            ..StepReport::default()
        };
        report.floor_load[b.feet[0]] = b.weight / 2.0;
        report.floor_load[b.feet[1]] = b.weight / 2.0;
        let r = read(&m, &b, &states, &report, floor);
        assert!(r.tilt.to_degrees() < 10.0, "{}", r.tilt.to_degrees());
        assert!(r.over_feet < 0.0, "{}", r.over_feet);
        assert_eq!(classify(&r, false), Label::Standing);
        // The facing is the toes' way; a target behind is out of reach.
        let ahead = reads(&r, Some(r.facing), 1500.0);
        assert!(ahead.can_aim && ahead.aim_off < 1.0 && ahead.can_push_up);
        let behind = reads(&r, Some(scale(r.facing, -1.0)), 1500.0);
        assert!(!behind.can_aim && behind.aim_off > 170.0);
    }
}
