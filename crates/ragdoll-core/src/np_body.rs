//! New Physics' body: FNV's own ragdoll (the 18 bodies and 17 joints of `skeleton.nif`,
//! [`crate::np_skeleton`]) simulated as rigid capsules, the joints held together and inside their
//! limits, each driven toward the animation by a spring and damper, standing on the floor.
//!
//! The solver is extended position based dynamics (XPBD: Müller, Macklin, Chentanez, Jeschke and
//! Kim, "Detailed Rigid Body Simulation with Extended Position Based Dynamics", 2020) with small
//! steps: each frame is cut into [`SUBSTEPS`]; in each the bodies move by their velocities, then
//! every constraint is projected once (joints, limits, drives, the root's support, the floor), and
//! the velocities are what the positions did; then a velocity pass damps the drives and the floor's
//! friction. Stiff joints and hard limits stay stable at any frame time, which a force-based
//! integrator with four substeps (the plan's first idea) did not promise.
//!
//! Units: metres, kilograms, seconds inside; the game's units (70 to a metre, [`UNITS`]) at the
//! edges ([`Model::states_from_nodes`], [`Model::node_worlds`]). A body's frame is its node's world
//! transform (FNV's bodies are plain `bhkRigidBody`s); its centre of mass lies at the data's
//! `center` in that frame.
//!
//! The joints: each child body pinned to its parent at the child's node origin (the data's pivot A,
//! zero for every joint), the parent's anchor where that origin lies in the parent's frame in the
//! bind pose: the drawn bones keep the skeleton's lengths (the data's arm pivots lie 1 to 2.2 units
//! off the nodes). The limits are the data's (cone, plane and twist; a hinge's axles together and
//! its angle), each widened to take in the animated pose's own value of it plus
//! [`LIMIT_MARGIN`]: the drawn pose follows its animation where the animation goes past the data
//! (the neck lies 20 degrees out of its cone in the bind pose; the arms' hinge axles 15 degrees
//! apart).

use crate::active::{m_from_q, qconj, qexp, qfrom_m, qlog, qmul, qnorm, qrot, Q, Q_ID};
use crate::flinch::{add, apply_m, cross, dot, length, mul, scale, sub, transpose, unit, M3, V3};
#[allow(unused_imports)]
use crate::fm::Fm;
use crate::np_skeleton::{Limit, Measure, Skeleton, HAVOK_UNITS};

/// Game units to a metre.
pub const UNITS: f32 = crate::UNITS_PER_METRE;
/// Gravity (m/s²).
pub const GRAVITY: f32 = 9.81;
/// Substeps a 60th of a second (the solver's step is a 960th whatever the frame's length:
/// [`SUBSTEP`]), and the most a frame takes.
pub const SUBSTEPS: usize = 16;
pub const SUBSTEP: f32 = 1.0 / (60.0 * SUBSTEPS as f32);
pub const MAX_SUBSTEPS: usize = 48;
/// How far past the animated pose's own value a limit is widened (radians, 3 degrees).
pub const LIMIT_MARGIN: f32 = 0.052;
/// Downed, the most a joint limit turns its joint back in a substep (rad; 9.6 rad/s at 16
/// substeps a 60th): hard for anything the solver's own steps put past it, never a snap.
pub const LIMIT_STEP: f32 = 0.01;
/// Brought inside the skeleton's own limits for the hand-over (`Tracking::data_limits`): at most
/// this a frame (rad, whatever its substeps): a few frames for a pose far out, never a turn over
/// 20 degrees a frame drawn.
pub const HAND_LIMIT_FRAME: f32 = 0.15;
/// The floor's friction (static, dynamic).
pub const FRICTION: (f32, f32) = (1.0, 0.8);
/// The longest frame stepped (s): a hitch is stepped as this.
pub const MAX_FRAME: f32 = 0.05;
/// The fastest a body moves (m/s) and turns (rad/s): a guard, as Havok's own, against a solver
/// upset flinging the body (a hit's impulse spins the struck body up to 150 rad/s for a substep
/// before the joints share it; a light neck whips past 170 at a fall's impact).
pub const MAX_SPEED: f32 = 30.0;
/// The most a surface pushes out what already lay inside it as a substep began (m/s): a floor
/// that came up into the body (its height changed between frames) lets it out at this, never in
/// one substep (Patrick's 803260d clip: the floor following the animated feet came up into them,
/// "feet 40912 N", and the body was launched two metres up). Sinking in this substep is pushed
/// out whole.
pub const MAX_OUT: f32 = 1.0;
pub const MAX_SPIN: f32 = 200.0;

/// A body: its node, mass, inertia about its centre of mass in its own frame, the centre of mass
/// and its capsule (in its own frame, metres).
#[derive(Clone, Debug)]
pub struct BodyDef {
    pub node: String,
    pub mass: f32,
    pub inv_mass: f32,
    pub inv_inertia: M3,
    pub com: V3,
    pub radius: f32,
    pub ends: [V3; 2],
}

/// A joint between a child body and its parent: the anchors (each body's frame, metres) and the
/// limit as the data has it.
#[derive(Clone, Debug)]
pub struct JointDef {
    pub child: usize,
    pub parent: usize,
    pub anchor: [V3; 2],
    pub limit: Limit,
    /// The joint's data (its measures and limits).
    pub data: crate::np_skeleton::Joint,
}

/// The ragdoll's make-up.
#[derive(Clone, Debug)]
pub struct Model {
    pub bodies: Vec<BodyDef>,
    pub joints: Vec<JointDef>,
    /// The body the others hang from (FNV's `Bip01 NonAccum`).
    pub root: usize,
    pub mass: f32,
    /// Each joint's subtree: the child and every body below it.
    pub below: Vec<Vec<usize>>,
    /// Each subtree's inertia about its joint in the bind pose (kg·m², a scalar), and the rest of
    /// the body's.
    pub below_inertia: Vec<f32>,
    pub above_inertia: Vec<f32>,
    /// Each joint's relative rotation in the bind pose (the skeleton's own rest: the game's
    /// ragdoll rests there, the neck 17 degrees out of its cone in the data).
    pub bind_rel: Vec<Q>,
}

/// A body's state: its centre of mass (world, metres), its frame's rotation, velocities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub x: V3,
    pub q: Q,
    pub v: V3,
    pub w: V3,
}

/// A joint's drive this frame: the child's rotation relative to its parent wanted (`parent⁻¹
/// child`, the animation's), the spring (N·m/rad) and damper (N·m·s/rad), and how much of the
/// weight hanging below the joint its muscles hold up (0 to 1: the torque that carries the
/// subtree's weight about the joint, so the spring answers only to what else moves it).
#[derive(Clone, Copy, Debug)]
pub struct Drive {
    pub target: Q,
    pub kp: f32,
    pub kd: f32,
    pub hold: f32,
}

/// Each body's turn tracked to its animation's this frame: the animation's angular velocity of each
/// body (rad/s, world) and the rate (1/s) at which the body's own turn off it decays. Per body, so
/// a light body between heavy ones does not limit it (a joint's damper is capped by the lighter
/// of its two bodies: FNV's 9 kg spine ball under 30 kg of chest, arms and head let the upper
/// body swing with a damping ratio of 0.23 whatever its damper).
#[derive(Clone, Debug, Default)]
pub struct Tracking {
    pub w: Vec<V3>,
    pub rate: Vec<f32>,
    /// Bodies the floor holds up but does not grip (no friction): an animated walk's feet,
    /// which go where the walk puts them.
    pub slip: Vec<usize>,
    /// Bodies that grip as if they carried the whole body's weight (the feet under the root's
    /// support, which stands in for the legs pushing through them): friction by the weight on
    /// every end within 2 mm of a surface, pushed or not; the rest by their contacts' pushes.
    pub grip: Vec<usize>,
    /// The joints' targets last frame (empty: none): each limit is widened by them too, so a
    /// target's change never leaves the pose outside the limits it held under (a downed body's
    /// joints switching from the animation to a rise's goals were snapped back in one substep).
    pub before: Vec<Q>,
    /// The relative rotations the limits are widened by in place of the drives' targets (empty:
    /// the targets): a downed body's goals (a rise's) are kept inside the limits of the data and
    /// its animation, never widening them.
    pub widen_by: Vec<Q>,
    /// A pose the limits take in too (empty: none): a downed living body's reach.
    pub reach: Vec<Q>,
    /// Limits brought back at most [`LIMIT_STEP`] a substep (a downed body, whose limits move
    /// under it as its goals change); otherwise at once.
    pub soft_limits: bool,
    /// With `soft_limits`, each joint's allowance (rad; empty or missing: none yet): how far out
    /// of its limit the joint may still lie, what it was found out by when the limits went soft,
    /// less what the soft pull has since brought in ([`StepReport::allow`] gives it back each
    /// frame). Only that is eased in: a joint pushed further out (a fall's impact folding the
    /// neck 50 degrees past its cone under a 0.01 rad soft pull) is held at once, and the
    /// soft pull's end never finds a joint out to snap in (the head 21 degrees in a frame).
    pub allow: Vec<f32>,
    /// Each body's own floor (world, metres; empty: the frame's flat floor for every body): the
    /// ground under it, a slope's included (a flat floor at one height pushed a downed body's
    /// shins and knees into a 20 degree slope: Patrick's 23:41 clip).
    pub floors: Vec<Plane>,
    /// Each body's floor under each of its ends (world, metres; empty: [`Tracking::floors`]): a
    /// foot across a step's edge rests its toe on the step and its heel below (one plane a body
    /// left its toe in the step's corner, 4.8 units in).
    pub end_floors: Vec<[Plane; 2]>,
    /// The edges of steps and ledges near the body (world, metres): a point on the edge, its
    /// direction (unit) and how far it reaches each way from the point. A capsule's end is kept
    /// its radius off each (a toe swinging over a slab's corner went 2.3 units into it: planes
    /// alone hold no corner).
    pub edges: Vec<Edge>,
    /// The skeleton's own limits only (the game's ragdoll's: none widened by the animation, the
    /// targets or a reach), brought in gently (with `soft_limits`): a body about to be handed to
    /// the game's ragdoll (whose constraints snapped a pose far outside them: the tester's 08:13
    /// clip, 79 to 183 degrees out).
    pub data_limits: bool,
    /// The skeleton's own limits as [`Tracking::data_limits`] has them, eased in at
    /// [`LIMIT_STEP`] (with `soft_limits`), not at the hand-over's pace: a downed body lying with
    /// no rise wished (`np::SETTLED_AFTER`), so that a death or a given-up rise finds it inside
    /// them already.
    pub settled_limits: bool,
    /// Each body's own walls (world, metres; empty: none): the sides of walls, crates and ledges
    /// found near it (a plane holds only the body it was found for, so a crate's face never
    /// reaches a body beyond the crate).
    pub body_walls: Vec<Vec<Plane>>,
    /// Each of [`Tracking::body_walls`]' tops (world, metres; missing or infinite: none known):
    /// a wall holds only what lies below its top (a step's face, 12 units high, held a toe
    /// resting on the step's top back off it at 1600 N).
    pub body_wall_tops: Vec<Vec<f32>>,
    /// Forces between two bodies this frame (N): the first body is pushed by the vector, the
    /// second by its opposite, every substep; the body's momentum as a whole is unchanged (a
    /// muscle between them: an arm straightening between its planted hand and its shoulder).
    pub pushes: Vec<(usize, usize, V3)>,
}

/// A pull on a body this frame (the root's support, a swinging foot's carry): a spring and damper
/// toward a pose (its centre of mass, world, metres; its rotation) moving at `v` (m/s): up and down
/// `kp`, `kd`, at most `max_force` (N); across `kp_h`, `kd_h`, at most `max_force_h`; its turn
/// `kp_turn`, `kd_turn`, at most `max_torque` (N·m).
#[derive(Clone, Copy, Debug)]
pub struct Support {
    pub body: usize,
    pub x: V3,
    pub v: V3,
    pub q: Q,
    pub kp: f32,
    pub kd: f32,
    pub max_force: f32,
    pub kp_h: f32,
    pub kd_h: f32,
    pub max_force_h: f32,
    pub kp_turn: f32,
    pub kd_turn: f32,
    pub max_torque: f32,
}

/// A flat surface the body rests on or leans against (a wall): a point on it and its normal out
/// of it (unit), metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub p: V3,
    pub n: V3,
}

/// A step's or a ledge's edge (world, metres): the segment through `p` along `dir` (unit),
/// `reach` each way, `p`'s height the higher ground's, which lies toward `up`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub p: V3,
    pub dir: V3,
    pub reach: f32,
    /// Flat, unit, across the edge toward the higher ground (the step's top).
    pub up: V3,
    /// How far toward `up` the edge's top reaches (metres; 0: the floors hold what lies over it
    /// and the bodies' own walls its face): a crate's, held as a box for every body, its top as
    /// a floor and its face under the edge as a wall (a torso thrown back over a crate's edge
    /// sank 8.4 units into its top; held up there, a thigh no cast had found the face for went
    /// 11 units in under it).
    pub top: f32,
}

/// What a frame's step found.
#[derive(Clone, Debug, Default)]
pub struct StepReport {
    /// Bodies touching the floor at the frame's end (with the contact's world point, metres).
    pub contacts: Vec<(usize, V3)>,
    /// Bodies touching a wall at the frame's end: the body, the contact's world point and the
    /// wall's index in the walls given.
    pub wall_contacts: Vec<(usize, V3, usize)>,
    /// The force each body's contacts carried over the frame (N, the frame's mean): from the floor,
    /// and from the walls (each body's sum).
    pub floor_load: Vec<f32>,
    pub wall_load: Vec<f32>,
    /// The most a joint lay outside its widened limit before the frame's last projection (rad).
    pub limit_most: f32,
    /// The largest joint gap at the end (m).
    pub gap_most: f32,
    /// Substeps in which a body was moving faster than [`MAX_SPEED`] or turning faster than
    /// [`MAX_SPIN`] and was slowed to it (a solver upset: none expected).
    pub clamped: u32,
    /// Each joint's allowance at the frame's end ([`Tracking::allow`]; with soft limits).
    pub allow: Vec<f32>,
}

impl Model {
    /// FNV's ragdoll from its skeleton's data.
    pub fn fnv() -> Model {
        Model::from_skeleton(&Skeleton::fnv())
    }

    pub fn from_skeleton(sk: &Skeleton) -> Model {
        let hu = HAVOK_UNITS / UNITS;
        let bind = sk.bind_worlds();
        let bodies: Vec<BodyDef> = sk
            .bodies
            .iter()
            .map(|b| {
                let ends = [scale(b.ends[0], hu), scale(b.ends[1], hu)];
                let com = scale(add(b.ends[0], b.ends[1]), 0.5 * hu);
                // At least a ball of `MIN_RADIUS` turns it (FNV's root body is an 8 kg ball of 4
                // cm under 40 kg of upper body: the solver could not slow the torso swinging on
                // it; a real pelvis turns like a ball of 10 cm or more).
                let inertia = floor_inertia(capsule_inertia(b.mass, b.radius * hu, ends), b.mass);
                BodyDef {
                    node: b.node.clone(),
                    mass: b.mass,
                    inv_mass: 1.0 / b.mass,
                    inv_inertia: invert(&inertia),
                    com,
                    radius: b.radius * hu,
                    ends,
                }
            })
            .collect();
        let index = |name: &str| bodies.iter().position(|b| b.node == name);
        let joints: Vec<JointDef> = sk
            .joints
            .iter()
            .filter_map(|j| {
                let (c, p) = (index(&j.child)?, index(&j.parent)?);
                let wc = bind[sk.node(&j.child)?];
                let wp = bind[sk.node(&j.parent)?];
                let at = apply_m(&transpose(&wp.r), sub(wc.t, wp.t));
                Some(JointDef {
                    child: c,
                    parent: p,
                    anchor: [[0.0; 3], scale(at, 1.0 / UNITS)],
                    limit: j.limit,
                    data: j.clone(),
                })
            })
            .collect();
        let root = index("Bip01 NonAccum").unwrap_or(0);
        let mass = bodies.iter().map(|b| b.mass).sum();
        let below = joints
            .iter()
            .map(|j: &JointDef| {
                let mut out = vec![j.child];
                let mut i = 0;
                while i < out.len() {
                    let at = out[i];
                    out.extend(joints.iter().filter(|k| k.parent == at).map(|k| k.child));
                    i += 1;
                }
                out
            })
            .collect();
        let mut model = Model {
            bodies,
            joints,
            root,
            mass,
            below,
            below_inertia: Vec::new(),
            above_inertia: Vec::new(),
            bind_rel: Vec::new(),
        };
        // The subtrees' inertia about their joints in the bind pose.
        let nodes: Vec<(M3, V3)> = model
            .bodies
            .iter()
            .map(|b| {
                let x = sk.node(&b.node).map(|i| bind[i]).unwrap_or(bind[0]);
                (x.r, x.t)
            })
            .collect();
        let states = model.states_from_nodes(&nodes);
        let about = |pivot: V3, inside: &dyn Fn(usize) -> bool| -> f32 {
            (0..model.bodies.len())
                .filter(|&b| inside(b))
                .map(|b| {
                    let d = length(sub(states[b].x, pivot));
                    let body = &model.bodies[b];
                    body.mass * d * d + 0.4 * body.mass * body.radius * body.radius
                })
                .sum()
        };
        let (mut below_i, mut above_i) = (Vec::new(), Vec::new());
        for (def, below) in model.joints.iter().zip(&model.below) {
            let pivot = scale(nodes[def.child].1, 1.0 / UNITS);
            below_i.push(about(pivot, &|b| below.contains(&b)));
            above_i.push(about(pivot, &|b| !below.contains(&b)));
        }
        model.below_inertia = below_i;
        model.above_inertia = above_i;
        model.bind_rel = model.relative(&states);
        model
    }

    /// `states` brought inside the skeleton's own limits (the bind pose's taken in) at once: the
    /// joints held together and each limit projected, `passes` times; answers the most a joint
    /// lay out before the last pass (rad). The pose handed to the game's ragdoll, whose
    /// constraints would snap it there.
    pub fn into_data_limits(&self, states: &mut [State], passes: usize) -> f32 {
        let mut most = 0.0;
        for _ in 0..passes {
            most = 0.0f32;
            for (j, def) in self.joints.iter().enumerate() {
                let bind = self.bind_rel.get(j).copied().unwrap_or(Q_ID);
                let lim = widen(def.limit, self.measure(j, bind), axle_apart(def, bind));
                self.pin(states, def);
                let mut none = f32::INFINITY;
                most = most.max(self.limit(states, def, &lim, false, LIMIT_STEP, &mut none));
                self.pin(states, def);
            }
        }
        most
    }

    /// `rel` (joint `j`'s relative rotation, `parent⁻¹ child`) brought inside the skeleton's own
    /// limits (the bind pose's taken in), as [`Model::into_data_limits`] brings a pose: a settled
    /// body's drive targets, so its muscles never pull a joint against the limits that hold it.
    pub fn into_data_limit(&self, states: &[State], j: usize, rel: Q) -> Q {
        let Some(def) = self.joints.get(j) else {
            return rel;
        };
        let bind = self.bind_rel.get(j).copied().unwrap_or(Q_ID);
        let lim = widen(def.limit, self.measure(j, bind), axle_apart(def, bind));
        let mut copy = states.to_vec();
        copy[def.parent].q = Q_ID;
        copy[def.child].q = rel;
        for _ in 0..4 {
            let mut none = f32::INFINITY;
            if self.limit(&mut copy, def, &lim, false, LIMIT_STEP, &mut none) <= 1e-4 {
                break;
            }
        }
        qnorm(qmul(qconj(copy[def.parent].q), copy[def.child].q))
    }

    /// How far each joint lies out of the skeleton's own limits (rad; the game's ragdoll's:
    /// cone, plane, twist or hinge, widened only by the bind pose), measured on a copy.
    pub fn data_limits_out(&self, states: &[State]) -> Vec<f32> {
        self.joints
            .iter()
            .enumerate()
            .map(|(j, def)| {
                let bind = self.bind_rel.get(j).copied().unwrap_or(Q_ID);
                let lim = widen(def.limit, self.measure(j, bind), axle_apart(def, bind));
                let mut copy = states.to_vec();
                let mut none = f32::INFINITY;
                self.limit(&mut copy, def, &lim, false, LIMIT_STEP, &mut none)
            })
            .collect()
    }

    /// The bind pose (each body's node's world, game units) the model was made from.
    pub fn bind(&self) -> (Skeleton, Vec<(M3, V3)>) {
        let sk = Skeleton::fnv();
        let w = sk.bind_worlds();
        let nodes = self
            .bodies
            .iter()
            .map(|b| {
                let x = sk.node(&b.node).map(|i| w[i]).unwrap_or(crate::pose::Xf {
                    r: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                    t: [0.0; 3],
                    s: 1.0,
                });
                (x.r, x.t)
            })
            .collect();
        (sk, nodes)
    }

    /// The whole body's inertia about the root body's centre (kg·m², a scalar) in the bind pose.
    pub fn inertia_about_root(&self) -> f32 {
        let (_, nodes) = self.bind();
        let s = self.states_from_nodes(&nodes);
        let c = s[self.root].x;
        self.bodies
            .iter()
            .zip(&s)
            .map(|(b, st)| {
                let d = length(sub(st.x, c));
                b.mass * d * d + b.mass * b.radius * b.radius
            })
            .sum()
    }

    /// The body whose node is `name`.
    pub fn body(&self, name: &str) -> Option<usize> {
        self.bodies.iter().position(|b| b.node == name)
    }

    /// The states of bodies whose nodes' worlds are `worlds` (each body's, game units), still.
    pub fn states_from_nodes(&self, worlds: &[(M3, V3)]) -> Vec<State> {
        self.bodies
            .iter()
            .zip(worlds)
            .map(|(b, (r, t))| {
                let r = crate::pose::ortho(r);
                State {
                    x: add(scale(*t, 1.0 / UNITS), apply_m(&r, b.com)),
                    q: qnorm(qfrom_m(&r)),
                    v: [0.0; 3],
                    w: [0.0; 3],
                }
            })
            .collect()
    }

    /// Each body's node's world (rotation, game units).
    pub fn node_worlds(&self, states: &[State]) -> Vec<(M3, V3)> {
        self.bodies
            .iter()
            .zip(states)
            .map(|(b, s)| {
                let r = m_from_q(s.q);
                (r, scale(sub(s.x, apply_m(&r, b.com)), UNITS))
            })
            .collect()
    }

    /// The whole body's centre of mass (metres) and velocity.
    pub fn centre_of_mass(&self, states: &[State]) -> (V3, V3) {
        let (mut x, mut v) = ([0.0; 3], [0.0; 3]);
        for (b, s) in self.bodies.iter().zip(states) {
            x = add(x, scale(s.x, b.mass));
            v = add(v, scale(s.v, b.mass));
        }
        (scale(x, 1.0 / self.mass), scale(v, 1.0 / self.mass))
    }

    /// Each joint's relative rotation (`parent⁻¹ child`) in `states`.
    pub fn relative(&self, states: &[State]) -> Vec<Q> {
        self.joints
            .iter()
            .map(|j| qnorm(qmul(qconj(states[j.parent].q), states[j.child].q)))
            .collect()
    }

    /// A joint measured from a relative rotation (`parent⁻¹ child`).
    pub fn measure(&self, j: usize, rel: Q) -> Measure {
        let id: M3 = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        self.joints[j].data.measure(&m_from_q(rel), &id)
    }

    /// A linear impulse `j` (N·s) on body `b` at the world point `at` (metres).
    pub fn impulse(&self, states: &mut [State], b: usize, at: V3, j: V3) {
        let s = &mut states[b];
        let def = &self.bodies[b];
        s.v = add(s.v, scale(j, def.inv_mass));
        let r = sub(at, s.x);
        let iw = world_inv_inertia(def, s.q);
        s.w = add(s.w, apply_m(&iw, cross(r, j)));
    }

    /// One frame of `dt` (s): the bodies under gravity, held by the joints and their widened
    /// limits (`targets`: each joint's animated relative rotation), driven by `drives`, the root
    /// held by `pulls` (the root's support, a swinging foot), standing on the floor at height
    /// `floor` (metres).
    pub fn step(
        &self,
        states: &mut [State],
        drives: &[Drive],
        pulls: &[Support],
        floor: f32,
        dt: f32,
    ) -> StepReport {
        self.step_tracked(states, drives, pulls, None, floor, dt)
    }

    /// [`Model::step`] with each body's turn tracked to its animation's (`tracking`).
    pub fn step_tracked(
        &self,
        states: &mut [State],
        drives: &[Drive],
        pulls: &[Support],
        tracking: Option<&Tracking>,
        floor: f32,
        dt: f32,
    ) -> StepReport {
        self.step_world(states, drives, pulls, tracking, floor, &[], dt)
    }

    /// [`Model::step_tracked`] with walls besides the floor (`walls`: the body's capsules kept on
    /// their outer side, with the floor's friction).
    #[allow(clippy::too_many_arguments)]
    pub fn step_world(
        &self,
        states: &mut [State],
        drives: &[Drive],
        pulls: &[Support],
        tracking: Option<&Tracking>,
        floor: f32,
        walls: &[Plane],
        dt: f32,
    ) -> StepReport {
        let mut report = StepReport {
            floor_load: vec![0.0; self.bodies.len()],
            wall_load: vec![0.0; self.bodies.len()],
            ..StepReport::default()
        };
        let mut surfaces = vec![Plane {
            p: [0.0, 0.0, floor],
            n: [0.0, 0.0, 1.0],
        }];
        surfaces.extend(
            walls
                .iter()
                .filter_map(|w| unit(w.n).map(|n| Plane { p: w.p, n })),
        );
        let dt = dt.clamp(0.0, MAX_FRAME);
        if dt <= 0.0 || states.len() != self.bodies.len() {
            return report;
        }
        // Substeps of one length whatever the frame's (`SUBSTEP`): the same body at 250 frames a
        // second as at 60 (a frame cut in 16 made a 4 ms frame's substeps a quarter as long,
        // and the recoil half as large).
        let substeps = ((dt / SUBSTEP).round() as usize).clamp(1, MAX_SUBSTEPS);
        let h = dt / substeps as f32;
        // The limits widened by the animated pose's own values.
        let widened: Vec<Limit> = self
            .joints
            .iter()
            .enumerate()
            .map(|(j, def)| {
                if tracking.is_some_and(|t| t.data_limits || t.settled_limits) {
                    // The skeleton's own, taking in its bind pose (the neck's lies 17 degrees out
                    // of its cone in the data: the game's ragdoll rests there).
                    let bind = self.bind_rel.get(j).copied().unwrap_or(Q_ID);
                    return widen(def.limit, self.measure(j, bind), axle_apart(def, bind));
                }
                let target = match tracking.and_then(|t| t.widen_by.get(j)) {
                    Some(&q) => q,
                    None => drives.get(j).map_or(Q_ID, |d| d.target),
                };
                let by_target = widen(def.limit, self.measure(j, target), axle_apart(def, target));
                let mut lim = by_target;
                for extra in [
                    tracking.and_then(|t| t.before.get(j)),
                    tracking.and_then(|t| t.reach.get(j)),
                ]
                .into_iter()
                .flatten()
                {
                    let both = widen(lim, self.measure(j, *extra), axle_apart(def, *extra));
                    lim = keep_apart(both, lim);
                }
                lim
            })
            .collect();
        let floors: &[Plane] = tracking.map_or(&[], |t| &t.floors);
        let end_floors: &[[Plane; 2]] = tracking.map_or(&[], |t| &t.end_floors);
        let edges: &[Edge] = tracking.map_or(&[], |t| &t.edges);
        let body_walls: &[Vec<Plane>] = tracking.map_or(&[], |t| &t.body_walls);
        let wall_tops: &[Vec<f32>] = tracking.map_or(&[], |t| &t.body_wall_tops);
        let mut allow: Vec<f32> = tracking.map_or(Vec::new(), |t| t.allow.clone());
        allow.resize(self.joints.len(), f32::INFINITY);
        for sub_i in 0..substeps {
            let prev: Vec<State> = states.to_vec();
            // The muscles' hold of the weight below each joint (a torque pair on the joint).
            for (j, def) in self.joints.iter().enumerate() {
                let Some(d) = drives.get(j).filter(|d| d.hold > 0.0) else {
                    continue;
                };
                let pivot = self.anchor_world(states, def, 0);
                let mut m = 0.0;
                let mut c = [0.0; 3];
                for &b in &self.below[j] {
                    m += self.bodies[b].mass;
                    c = add(c, scale(states[b].x, self.bodies[b].mass));
                }
                let c = scale(c, 1.0 / m);
                let tau = scale(cross(sub(c, pivot), [0.0, 0.0, m * GRAVITY]), d.hold * h);
                let ic = world_inv_inertia(&self.bodies[def.child], states[def.child].q);
                let ip = world_inv_inertia(&self.bodies[def.parent], states[def.parent].q);
                states[def.child].w = add(states[def.child].w, apply_m(&ic, tau));
                states[def.parent].w = sub(states[def.parent].w, apply_m(&ip, tau));
            }
            // The muscles between bodies (equal and opposite).
            if let Some(t) = tracking {
                for &(a, b, f) in &t.pushes {
                    if let (Some(da), Some(db)) = (self.bodies.get(a), self.bodies.get(b)) {
                        states[a].v = add(states[a].v, scale(f, da.inv_mass * h));
                        states[b].v = sub(states[b].v, scale(f, db.inv_mass * h));
                    }
                }
            }
            // Move.
            for (s, def) in states.iter_mut().zip(&self.bodies) {
                s.v[2] -= GRAVITY * h;
                s.x = add(s.x, scale(s.v, h));
                let iw = world_inv_inertia(def, s.q);
                let inertia = invert(&iw);
                // The gyroscopic term (`I⁻¹ (ω × I ω)`), small at these speeds.
                let gyro = apply_m(&iw, cross(s.w, apply_m(&inertia, s.w)));
                s.w = sub(s.w, scale(gyro, h));
                s.q = qnorm(add_q(
                    s.q,
                    scale_q(qmul([0.0, s.w[0], s.w[1], s.w[2]], s.q), 0.5 * h),
                ));
            }
            // The joints: together, inside their limits, driven.
            for (j, def) in self.joints.iter().enumerate() {
                self.pin(states, def);
                let soft = tracking.is_some_and(|t| t.soft_limits);
                let step = if tracking.is_some_and(|t| t.data_limits) {
                    HAND_LIMIT_FRAME / substeps as f32
                } else {
                    LIMIT_STEP
                };
                let out = self.limit(states, def, &widened[j], soft, step, &mut allow[j]);
                if sub_i == substeps - 1 {
                    report.limit_most = report.limit_most.max(out);
                }
                if let Some(d) = drives.get(j) {
                    if d.kp > 0.0 {
                        let target = qmul(states[def.parent].q, d.target);
                        let c = qlog(qmul(target, qconj(states[def.child].q)));
                        self.turn(states, def.child, Some(def.parent), c, 1.0 / d.kp, h);
                    }
                }
                self.pin(states, def);
            }
            for sp in pulls {
                self.support(states, sp, h);
            }
            let slip: &[usize] = tracking.map_or(&[], |t| &t.slip);
            let grip: &[usize] = tracking.map_or(&[], |t| &t.grip);
            let mut loads = self.surfaces(
                states,
                &prev,
                &surfaces,
                (floors, end_floors),
                (body_walls, wall_tops, edges),
                slip,
                h,
            );
            loads.extend(self.edge_contacts(states, &prev, edges, h));
            // Each surface's push this substep as a force (N), the frame's mean.
            let to_force = 1.0 / (h * h * substeps as f32);
            for &(b, k, lambda, _, _) in &loads {
                if k == 0 {
                    report.floor_load[b] += lambda * to_force;
                } else {
                    report.wall_load[b] += lambda * to_force;
                }
            }
            // Velocities from the moves.
            for (s, p) in states.iter_mut().zip(&prev) {
                s.v = scale(sub(s.x, p.x), 1.0 / h);
                let dq = qmul(s.q, qconj(p.q));
                let w = scale([dq[1], dq[2], dq[3]], 2.0 / h);
                s.w = if dq[0] < 0.0 { scale(w, -1.0) } else { w };
            }
            // The guard against an upset: no body faster than the most.
            let mut over = false;
            for st in states.iter_mut() {
                let v = length(st.v);
                if v > MAX_SPEED {
                    st.v = scale(st.v, MAX_SPEED / v);
                    over = true;
                }
                let w = length(st.w);
                if w > MAX_SPIN {
                    st.w = scale(st.w, MAX_SPIN / w);
                    over = true;
                }
            }
            if over {
                report.clamped += 1;
            }
            // No bounce (the paper's restitution, at 0): a contact's push out of a surface moves
            // the body out but leaves it no speed away from it.
            for &(b, _, _, local, n) in &loads {
                // A sphere-like body's push was through its centre: so is its stop.
                let r = if is_sphere(&self.bodies[b]) {
                    [0.0; 3]
                } else {
                    sub(qrot(states[b].q, local), scale(n, self.bodies[b].radius))
                };
                let v = add(states[b].v, cross(states[b].w, r));
                let vn = dot(v, n);
                if vn > 0.0 {
                    let w = self.point_weight(states, b, r, n);
                    let p = scale(n, -vn / w.max(1e-6));
                    let s_ = &mut states[b];
                    s_.v = add(s_.v, scale(p, self.bodies[b].inv_mass));
                    let iw = world_inv_inertia(&self.bodies[b], s_.q);
                    s_.w = add(s_.w, apply_m(&iw, cross(r, p)));
                }
            }
            // The drives' damping (the paper's joint damping, between the two bodies).
            for (j, def) in self.joints.iter().enumerate() {
                if let Some(d) = drives.get(j).filter(|d| d.kd > 0.0) {
                    self.damp_turn(states, def.child, Some(def.parent), d.kd, h);
                }
            }
            // Each body's turn off its animation's decays at its tracking rate.
            if let Some(tr) = tracking {
                for (b, st) in states.iter_mut().enumerate() {
                    let (Some(&aw), Some(&rate)) = (tr.w.get(b), tr.rate.get(b)) else {
                        continue;
                    };
                    let k = 1.0 - (-rate * h).dexp();
                    st.w = add(st.w, scale(sub(aw, st.w), k));
                }
            }
            // The support's damping, the floor's sliding friction.
            for sp in pulls {
                let r = sp.body;
                self.damp_turn(states, r, None, sp.kd_turn, h);
                let m = self.bodies[r].inv_mass * h;
                let (kv, kh) = ((sp.kd * m).min(1.0), (sp.kd_h * m).min(1.0));
                let rel = sub(states[r].v, sp.v);
                let dv = [-rel[0] * kh, -rel[1] * kh, -rel[2] * kv];
                states[r].v = add(states[r].v, dv);
            }
            self.friction(states, &loads, h, slip, grip);
            self.grip_friction(states, &surfaces, (floors, end_floors), h, grip);
        }
        for (i, def) in self.bodies.iter().enumerate() {
            for (end, e) in def.ends.into_iter().enumerate() {
                let fl = end_floor(&surfaces, (floors, end_floors), i, end);
                let p = add(states[i].x, qrot(states[i].q, sub(e, def.com)));
                let d = dot(sub(p, fl.p), fl.n) - def.radius;
                if d <= 0.005 {
                    report
                        .contacts
                        .push((i, sub(p, scale(fl.n, d + def.radius))));
                }
                let own: &[Plane] = body_walls.get(i).map_or(&[], |w| w.as_slice());
                let tops: &[f32] = wall_tops.get(i).map_or(&[], |w| w.as_slice());
                for (k, w) in surfaces.iter().chain(own).enumerate().skip(1) {
                    if k >= surfaces.len()
                        && tops.get(k - surfaces.len()).is_some_and(|&top| p[2] > top)
                    {
                        continue;
                    }
                    let d = dot(sub(p, w.p), w.n) - def.radius;
                    if d <= 0.005 {
                        report
                            .wall_contacts
                            .push((i, sub(p, scale(w.n, d + def.radius)), k - 1));
                    }
                }
            }
        }
        report.allow = allow;
        report.gap_most = self
            .joints
            .iter()
            .map(|def| {
                length(sub(
                    self.anchor_world(states, def, 0),
                    self.anchor_world(states, def, 1),
                ))
            })
            .fold(0.0, f32::max);
        report
    }

    fn anchor_world(&self, states: &[State], def: &JointDef, side: usize) -> V3 {
        let b = if side == 0 { def.child } else { def.parent };
        let s = &states[b];
        add(s.x, qrot(s.q, sub(def.anchor[side], self.bodies[b].com)))
    }

    /// The joint's two anchors brought together (hard).
    fn pin(&self, states: &mut [State], def: &JointDef) {
        let pa = self.anchor_world(states, def, 0);
        let pb = self.anchor_world(states, def, 1);
        let d = sub(pb, pa);
        let c = length(d);
        if c < 1e-7 {
            return;
        }
        let n = scale(d, 1.0 / c);
        let ra = sub(pa, states[def.child].x);
        let rb = sub(pb, states[def.parent].x);
        let wa = self.point_weight(states, def.child, ra, n);
        let wb = self.point_weight(states, def.parent, rb, n);
        let lambda = c / (wa + wb);
        let p = scale(n, lambda);
        self.move_at(states, def.child, ra, p);
        self.move_at(states, def.parent, rb, scale(p, -1.0));
    }

    /// The joint brought inside its (widened) limit; answers how far out it was (rad).
    fn limit(
        &self,
        states: &mut [State],
        def: &JointDef,
        lim: &Limit,
        soft: bool,
        step: f32,
        allow: &mut f32,
    ) -> f32 {
        let (ra, rb) = (
            m_from_q(states[def.child].q),
            m_from_q(states[def.parent].q),
        );
        let mut most = 0.0f32;
        let mut corrections: Vec<V3> = Vec::new();
        match *lim {
            Limit::Ragdoll {
                twist,
                plane,
                cone,
                planes,
                twists,
                ..
            } => {
                let (ta, tb) = (apply_m(&ra, twist[0]), apply_m(&rb, twist[1]));
                let pl_b = apply_m(&rb, plane[1]);
                let phi = dot(ta, tb).clamp(-1.0, 1.0).dacos();
                if phi > cone {
                    if let Some(n) = unit(cross(ta, tb)) {
                        corrections.push(scale(n, phi - cone));
                        most = most.max(phi - cone);
                    }
                }
                let s = dot(ta, pl_b).clamp(-1.0, 1.0).dasin();
                if let Some(k) = unit(cross(ta, pl_b)) {
                    if s > planes.1 {
                        corrections.push(scale(k, -(s - planes.1)));
                        most = most.max(s - planes.1);
                    } else if s < planes.0 {
                        corrections.push(scale(k, planes.0 - s));
                        most = most.max(planes.0 - s);
                    }
                }
                let axis = unit(add(ta, tb)).unwrap_or(ta);
                let flat = |v: V3| unit(sub(v, scale(axis, dot(v, axis))));
                if let (Some(pa), Some(pb)) =
                    (flat(apply_m(&ra, plane[0])), flat(apply_m(&rb, plane[1])))
                {
                    let tau = dot(cross(pb, pa), axis).datan2(dot(pb, pa));
                    if tau > twists.1 {
                        corrections.push(scale(axis, -(tau - twists.1)));
                        most = most.max(tau - twists.1);
                    } else if tau < twists.0 {
                        corrections.push(scale(axis, twists.0 - tau));
                        most = most.max(twists.0 - tau);
                    }
                }
            }
            Limit::Hinge {
                axle,
                perp1,
                angles,
                perp2,
            } => {
                // `perp2` carries the axles' allowed angle apart ([`widen`]).
                let apart_most = perp2[0][0];
                let (aa, ab) = (apply_m(&ra, axle[0]), apply_m(&rb, axle[1]));
                let apart = dot(aa, ab).clamp(-1.0, 1.0).dacos();
                if apart > apart_most {
                    if let Some(n) = unit(cross(aa, ab)) {
                        corrections.push(scale(n, apart - apart_most));
                        most = most.max(apart - apart_most);
                    }
                }
                let (pa, pb) = (apply_m(&ra, perp1[0]), apply_m(&rb, perp1[1]));
                let theta = dot(cross(pb, pa), aa).datan2(dot(pb, pa));
                if theta > angles.1 {
                    corrections.push(scale(aa, -(theta - angles.1)));
                    most = most.max(theta - angles.1);
                } else if theta < angles.0 {
                    corrections.push(scale(aa, angles.0 - theta));
                    most = most.max(angles.0 - theta);
                }
            }
        }
        let mut left = 0.0f32;
        for c in corrections {
            // Soft, at most `step` a substep of what the joint's allowance holds (a pose found
            // far outside as its widening moved under it is brought back within a few frames,
            // not snapped in one substep), and whatever lies beyond the allowance at up to four
            // times that (an impact's fold held within a few substeps, not snapped).
            let a = length(c);
            let take = if soft {
                (a - *allow).min(4.0 * step).max(a.min(step)).min(a)
            } else {
                a
            };
            left = left.max(a - take);
            let c = if take < a {
                scale(c, take / a.max(1e-9))
            } else {
                c
            };
            self.turn(states, def.child, Some(def.parent), c, 0.0, 1.0);
        }
        *allow = if soft { allow.min(left) } else { f32::INFINITY };
        most
    }

    /// Body `a` turned by `c` (a world rotation vector) against body `b` (or the world), each by
    /// its share; `compliance` (rad/(N·m)) makes it a spring over the substep `h`.
    fn turn(
        &self,
        states: &mut [State],
        a: usize,
        b: Option<usize>,
        c: V3,
        compliance: f32,
        h: f32,
    ) {
        let theta = length(c);
        if !(theta > 1e-7 && theta.is_finite()) {
            return;
        }
        let n = scale(c, 1.0 / theta);
        let wa = dot(
            n,
            apply_m(&world_inv_inertia(&self.bodies[a], states[a].q), n),
        );
        let wb = b.map_or(0.0, |b| {
            dot(
                n,
                apply_m(&world_inv_inertia(&self.bodies[b], states[b].q), n),
            )
        });
        let alpha = compliance / (h * h);
        let lambda = theta / (wa + wb + alpha);
        let p = scale(n, lambda);
        self.rotate(states, a, p);
        if let Some(b) = b {
            self.rotate(states, b, scale(p, -1.0));
        }
    }

    /// A body pulled toward its pull's pose: capped springs, up and down and across.
    fn support(&self, states: &mut [State], sp: &Support, h: f32) {
        let r = sp.body;
        let d = sub(sp.x, states[r].x);
        let w = self.bodies[r].inv_mass;
        for (part, kp, most) in [
            ([0.0, 0.0, d[2]], sp.kp, sp.max_force),
            ([d[0], d[1], 0.0], sp.kp_h, sp.max_force_h),
        ] {
            let c = length(part);
            if c > 1e-7 && kp > 0.0 {
                let n = scale(part, 1.0 / c);
                let alpha = 1.0 / (kp * h * h);
                let lambda = (c / (w + alpha)).min(most * h * h);
                states[r].x = add(states[r].x, scale(n, lambda * w));
            }
        }
        if sp.kp_turn > 0.0 {
            let c = qlog(qmul(sp.q, qconj(states[r].q)));
            let theta = length(c);
            if theta > 1e-7 {
                let n = scale(c, 1.0 / theta);
                let w = dot(
                    n,
                    apply_m(&world_inv_inertia(&self.bodies[r], states[r].q), n),
                );
                let alpha = 1.0 / (sp.kp_turn * h * h);
                let lambda = (theta / (w + alpha)).min(sp.max_torque * h * h);
                self.rotate(states, r, scale(n, lambda));
            }
        }
    }

    /// Each capsule end kept its radius off each edge (nearest point of the segment), as
    /// [`Model::surfaces`] keeps it off a plane: what lay inside as the substep began let out at
    /// [`MAX_OUT`] at most. Its loads count as walls'.
    fn edge_contacts(
        &self,
        states: &mut [State],
        prev: &[State],
        edges: &[Edge],
        h: f32,
    ) -> Vec<(usize, usize, f32, V3, V3)> {
        let mut loads = Vec::new();
        if edges.is_empty() {
            return loads;
        }
        let nearest = |c: V3, ed: &Edge| {
            let t = dot(sub(c, ed.p), ed.dir).clamp(-ed.reach, ed.reach);
            add(ed.p, scale(ed.dir, t))
        };
        for (i, def) in self.bodies.iter().enumerate() {
            for e in def.ends {
                let local = sub(e, def.com);
                for (k, ed) in edges.iter().enumerate() {
                    let centre = add(states[i].x, qrot(states[i].q, local));
                    // A box's edge: over its top held up by the top, under the edge held out
                    // by its face, as a box holds it ([`box_side`]).
                    let was = add(prev[i].x, qrot(prev[i].q, local));
                    if let Some((n, gap)) = box_side(centre, was, ed) {
                        let depth = def.radius - gap;
                        if depth <= 0.0 {
                            continue;
                        }
                        let inside = box_side(was, was, ed)
                            .filter(|w| dot(w.0, n) > 0.5)
                            .map_or(0.0, |w| (def.radius - w.1).max(0.0));
                        let depth = depth.min((depth - inside).max(0.0) + inside.min(MAX_OUT * h));
                        if depth <= 0.0 {
                            continue;
                        }
                        let r = if is_sphere(def) {
                            scale(n, -def.radius)
                        } else {
                            sub(sub(centre, scale(n, def.radius)), states[i].x)
                        };
                        let w = self.point_weight(states, i, r, n);
                        self.move_at(states, i, r, scale(n, depth / w));
                        loads.push((i, usize::MAX - k, depth / w, local, n));
                        continue;
                    }
                    let off = sub(centre, nearest(centre, ed));
                    let d = length(off);
                    if d >= def.radius {
                        continue;
                    }
                    let n = if d > 1e-6 {
                        scale(off, 1.0 / d)
                    } else {
                        [0.0, 0.0, 1.0]
                    };
                    let was = add(prev[i].x, qrot(prev[i].q, local));
                    let inside = (def.radius - length(sub(was, nearest(was, ed)))).max(0.0);
                    let depth = def.radius - d;
                    let depth = depth.min((depth - inside).max(0.0) + inside.min(MAX_OUT * h));
                    if depth <= 0.0 {
                        continue;
                    }
                    // A sphere-like body (the head) is pushed out through its centre: its ends
                    // lie inside its radius of it, so a push at an end's contact point only
                    // spun it against the ground ([`is_sphere`]).
                    let r = if is_sphere(def) {
                        scale(n, -def.radius)
                    } else {
                        sub(sub(centre, scale(n, def.radius)), states[i].x)
                    };
                    let w = self.point_weight(states, i, r, n);
                    self.move_at(states, i, r, scale(n, depth / w));
                    loads.push((i, usize::MAX - k, depth / w, local, n));
                }
            }
        }
        loads
    }

    /// The capsules kept on the outer side of each surface (the floor first, then the walls); a
    /// contact that has not slid past the static friction is held where it was. Answers each push
    /// (body, surface, its size: kg·m, the capsule end's centre in the body's turn, the surface's
    /// normal).
    #[allow(clippy::type_complexity)]
    #[allow(clippy::too_many_arguments)]
    fn surfaces(
        &self,
        states: &mut [State],
        prev: &[State],
        surfaces: &[Plane],
        floors: (&[Plane], &[[Plane; 2]]),
        (body_walls, wall_tops, edges): (&[Vec<Plane>], &[Vec<f32>], &[Edge]),
        slip: &[usize],
        h: f32,
    ) -> Vec<(usize, usize, f32, V3, V3)> {
        let mut loads = Vec::new();
        for (i, def) in self.bodies.iter().enumerate() {
            let grips = !slip.contains(&i);
            let own: &[Plane] = body_walls.get(i).map_or(&[], |w| w.as_slice());
            let tops: &[f32] = wall_tops.get(i).map_or(&[], |w| w.as_slice());
            let shared = surfaces.len();
            for (end, e) in def.ends.into_iter().enumerate() {
                let fl = end_floor(surfaces, floors, i, end);
                let local = sub(e, def.com);
                for (k, sf) in surfaces.iter().chain(own).enumerate() {
                    let sf = if k == 0 { &fl } else { sf };
                    let n = sf.n;
                    let centre = add(states[i].x, qrot(states[i].q, local));
                    if k >= shared && tops.get(k - shared).is_some_and(|&top| centre[2] > top) {
                        continue;
                    }
                    // A box's face: its edge's ([`box_side`]).
                    if k >= shared
                        && tops.get(k - shared).is_some_and(|&top| {
                            top.is_finite() && boxed_wall(sf, top, edges, centre)
                        })
                    {
                        continue;
                    }
                    let depth = def.radius - dot(sub(centre, sf.p), n);
                    if depth <= 0.0 {
                        continue;
                    }
                    // What lay inside as the substep began is let out at [`MAX_OUT`] at most;
                    // what sank in since, whole.
                    let was = add(prev[i].x, qrot(prev[i].q, local));
                    let inside = (def.radius - dot(sub(was, sf.p), n)).max(0.0);
                    let depth = depth.min((depth - inside).max(0.0) + inside.min(MAX_OUT * h));
                    if depth <= 0.0 {
                        continue;
                    }
                    let p = sub(centre, scale(n, def.radius));
                    let r = sub(p, states[i].x);
                    let w = self.point_weight(states, i, r, n);
                    let lambda = depth / w;
                    self.move_at(states, i, r, scale(n, lambda));
                    loads.push((i, k, lambda, local, n));
                    // Static friction: the contact point's slide along the surface this substep
                    // taken back while it is within the friction cone of the push out.
                    let now = add(states[i].x, qrot(states[i].q, local));
                    let was = add(prev[i].x, qrot(prev[i].q, local));
                    let slide = sub(now, was);
                    let slide = sub(slide, scale(n, dot(slide, n)));
                    let s_len = length(slide);
                    if grips
                        && !(is_sphere(def) && s_len > SPHERE_SKID * h)
                        && s_len > 1e-7
                        && s_len < FRICTION.0 * depth
                    {
                        let t = scale(slide, -1.0 / s_len);
                        let r = if is_sphere(def) {
                            [0.0; 3]
                        } else {
                            sub(sub(now, scale(n, def.radius)), states[i].x)
                        };
                        let w = self.point_weight(states, i, r, t);
                        self.move_at(states, i, r, scale(t, s_len / w));
                    }
                }
            }
        }
        loads
    }

    /// Each contact's sliding friction on the velocities (Coulomb's, the paper's): the contact
    /// point's slide along its surface slowed by at most the friction of the push that held it
    /// out this substep.
    #[allow(clippy::type_complexity)]
    fn friction(
        &self,
        states: &mut [State],
        loads: &[(usize, usize, f32, V3, V3)],
        h: f32,
        slip: &[usize],
        grip: &[usize],
    ) {
        for &(i, _, lambda, local, n) in loads {
            if slip.contains(&i) || grip.contains(&i) || lambda <= 0.0 {
                continue;
            }
            let def = &self.bodies[i];
            // A sphere-like body's contact point is under its centre, as it is pushed out.
            let r = if is_sphere(def) {
                [0.0; 3]
            } else {
                sub(qrot(states[i].q, local), scale(n, def.radius))
            };
            let v = add(states[i].v, cross(states[i].w, r));
            let vt = sub(v, scale(n, dot(v, n)));
            let s = length(vt);
            if s < 1e-6 {
                continue;
            }
            let t = scale(vt, -1.0 / s);
            let w = self.point_weight(states, i, r, t).max(1e-6);
            // At most what stops the slide, at most the friction of the push out.
            let mu = if is_sphere(def) && s > SPHERE_SKID {
                FRICTION.1 * SPHERE_FRICTION
            } else {
                FRICTION.1
            };
            let impulse = (s / w).min(mu * lambda / h);
            let p = scale(t, impulse);
            let s_ = &mut states[i];
            s_.v = add(s_.v, scale(p, self.bodies[i].inv_mass));
            let iw = world_inv_inertia(&self.bodies[i], s_.q);
            s_.w = add(s_.w, apply_m(&iw, cross(r, p)));
        }
    }

    /// The gripping bodies' friction (the standing feet): each capsule end touching a surface
    /// (within 2 mm) has its contact point's slide slowed by the weight's friction, pushed or
    /// not (the root's support carries the weight the legs put through them).
    fn grip_friction(
        &self,
        states: &mut [State],
        surfaces: &[Plane],
        floors: (&[Plane], &[[Plane; 2]]),
        h: f32,
        grip: &[usize],
    ) {
        for &i in grip {
            let Some(def) = self.bodies.get(i) else {
                continue;
            };
            for (end, e) in def.ends.into_iter().enumerate() {
                let fl = end_floor(surfaces, floors, i, end);
                let local = sub(e, def.com);
                for (k, sf) in surfaces.iter().enumerate() {
                    let sf = if k == 0 { &fl } else { sf };
                    let n = sf.n;
                    let centre = add(states[i].x, qrot(states[i].q, local));
                    if dot(sub(centre, sf.p), n) - def.radius > 0.002 {
                        continue;
                    }
                    let r = sub(sub(centre, scale(n, def.radius)), states[i].x);
                    let v = add(states[i].v, cross(states[i].w, r));
                    let vt = sub(v, scale(n, dot(v, n)));
                    let s = length(vt);
                    if s < 1e-6 {
                        continue;
                    }
                    let dv = (FRICTION.1 * GRAVITY * h).min(s);
                    let t = scale(vt, -1.0 / s);
                    let w = self.point_weight(states, i, r, t);
                    let p = scale(t, dv / w.max(1e-6));
                    let s_ = &mut states[i];
                    s_.v = add(s_.v, scale(p, self.bodies[i].inv_mass));
                    let iw = world_inv_inertia(&self.bodies[i], s_.q);
                    s_.w = add(s_.w, apply_m(&iw, cross(r, p)));
                }
            }
        }
    }

    /// A drive's damping on the turn of `a` against `b` (or the world): at most what stops it.
    fn damp_turn(&self, states: &mut [State], a: usize, b: Option<usize>, kd: f32, h: f32) {
        if kd <= 0.0 {
            return;
        }
        let rel = match b {
            Some(b) => sub(states[a].w, states[b].w),
            None => states[a].w,
        };
        let s = length(rel);
        if s < 1e-7 {
            return;
        }
        let n = scale(rel, 1.0 / s);
        let ia = world_inv_inertia(&self.bodies[a], states[a].q);
        let wa = dot(n, apply_m(&ia, n));
        let ib = b.map(|b| world_inv_inertia(&self.bodies[b], states[b].q));
        let wb = ib.map_or(0.0, |i| dot(n, apply_m(&i, n)));
        let impulse = (kd * h * s).min(s / (wa + wb));
        let p = scale(n, -impulse);
        states[a].w = add(states[a].w, apply_m(&ia, p));
        if let (Some(b), Some(ib)) = (b, ib) {
            states[b].w = sub(states[b].w, apply_m(&ib, p));
        }
    }

    fn point_weight(&self, states: &[State], b: usize, r: V3, n: V3) -> f32 {
        let rn = cross(r, n);
        self.bodies[b].inv_mass
            + dot(
                rn,
                apply_m(&world_inv_inertia(&self.bodies[b], states[b].q), rn),
            )
    }

    fn move_at(&self, states: &mut [State], b: usize, r: V3, p: V3) {
        let s = &mut states[b];
        s.x = add(s.x, scale(p, self.bodies[b].inv_mass));
        let dw = apply_m(&world_inv_inertia(&self.bodies[b], s.q), cross(r, p));
        s.q = qnorm(add_q(
            s.q,
            scale_q(qmul([0.0, dw[0], dw[1], dw[2]], s.q), 0.5),
        ));
    }

    fn rotate(&self, states: &mut [State], b: usize, p: V3) {
        let s = &mut states[b];
        let dw = apply_m(&world_inv_inertia(&self.bodies[b], s.q), p);
        s.q = qnorm(add_q(
            s.q,
            scale_q(qmul([0.0, dw[0], dw[1], dw[2]], s.q), 0.5),
        ));
    }
}

/// A limit widened to take in a pose's own values (`at`, and for a hinge its axles' angle apart
/// `apart`) plus [`LIMIT_MARGIN`]. A hinge's allowed axle angle travels in `perp2[0][0]`.
fn widen(lim: Limit, at: Measure, apart: f32) -> Limit {
    let m = LIMIT_MARGIN;
    match (lim, at) {
        (
            Limit::Ragdoll {
                twist,
                plane,
                motor,
                cone,
                planes,
                twists,
            },
            Measure::Ragdoll {
                cone: c,
                plane: p,
                twist: t,
            },
        ) => Limit::Ragdoll {
            twist,
            plane,
            motor,
            cone: cone.max(c + m),
            planes: (planes.0.min(p - m), planes.1.max(p + m)),
            twists: (twists.0.min(t - m), twists.1.max(t + m)),
        },
        (
            Limit::Hinge {
                axle,
                perp1,
                mut perp2,
                angles,
            },
            Measure::Hinge { angle },
        ) => {
            perp2[0] = [apart + m, 0.0, 0.0];
            Limit::Hinge {
                axle,
                perp1,
                perp2,
                angles: (angles.0.min(angle - m), angles.1.max(angle + m)),
            }
        }
        (lim, _) => lim,
    }
}

/// `lim` with a hinge's allowed axle angle the larger of its own and `other`'s.
fn keep_apart(lim: Limit, other: Limit) -> Limit {
    match (lim, other) {
        (
            Limit::Hinge {
                axle,
                perp1,
                mut perp2,
                angles,
            },
            Limit::Hinge { perp2: o, .. },
        ) => {
            perp2[0][0] = perp2[0][0].max(o[0][0]);
            Limit::Hinge {
                axle,
                perp1,
                perp2,
                angles,
            }
        }
        (lim, _) => lim,
    }
}

/// A hinge's axles' angle apart at the relative rotation `rel` (0 for a ragdoll joint).
fn axle_apart(def: &JointDef, rel: Q) -> f32 {
    match def.limit {
        Limit::Hinge { axle, .. } => {
            let aa = qrot(rel, axle[0]);
            dot(aa, axle[1]).clamp(-1.0, 1.0).dacos()
        }
        _ => 0.0,
    }
}

fn add_q(a: Q, b: Q) -> Q {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}
fn scale_q(a: Q, k: f32) -> Q {
    [a[0] * k, a[1] * k, a[2] * k, a[3] * k]
}

/// A body's inverse inertia in the world at rotation `q`.
fn world_inv_inertia(b: &BodyDef, q: Q) -> M3 {
    let r = m_from_q(q);
    mul(&mul(&r, &b.inv_inertia), &transpose(&r))
}

/// A capsule's inertia about its centre in its frame: mass `m`, radius `r`, the spheres' centres
/// `ends` (uniform density).
fn capsule_inertia(m: f32, r: f32, ends: [V3; 2]) -> M3 {
    let axis = sub(ends[0], ends[1]);
    let l = length(axis);
    let u = unit(axis).unwrap_or([1.0, 0.0, 0.0]);
    let vc = std::f32::consts::PI * r * r * l;
    let vh = 4.0 / 3.0 * std::f32::consts::PI * r * r * r;
    let mc = m * vc / (vc + vh);
    let mh = m - mc;
    let along = mc * r * r / 2.0 + mh * 2.0 * r * r / 5.0;
    let across = mc * (l * l / 12.0 + r * r / 4.0)
        + mh * (2.0 * r * r / 5.0 + l * l / 4.0 + 3.0 * l * r / 8.0);
    let mut out = [0.0f32; 9];
    for i in 0..3 {
        for k in 0..3 {
            let id = if i == k { 1.0 } else { 0.0 };
            out[i * 3 + k] = across * (id - u[i] * u[k]) + along * u[i] * u[k];
        }
    }
    out
}

/// The smallest a body's turn's inertia is taken as: a ball of this radius (m).
pub const MIN_RADIUS: f32 = 0.08;

/// `inertia` with every principal value at least a [`MIN_RADIUS`] ball's of mass `m` (added on
/// the diagonal: the capsule's own axes are kept).
/// A point's place in a box edge's frame ([`Edge::top`]): along the edge from its middle,
/// behind the face (toward `up`), and over the top (metres).
fn box_local(c: V3, ed: &Edge) -> V3 {
    let rel = sub(c, ed.p);
    [dot(rel, ed.dir), dot(rel, ed.up), rel[2]]
}

/// A box ([`Edge::top`]: its footprint the rectangle `reach` each way along the edge and `top`
/// behind it, its top at the edge's height, its sides down to the ground): the surface holding
/// a capsule end's centre `c` (at `was` as the substep began) and how far `c` lies out of it
/// (metres; under 0 inside). Outside, the nearest point of the box (a face, an edge, a corner:
/// a thigh at a crate's upright corner, where no plane reached, lay 3.7 units in it); inside,
/// out by the side it came in by (where `was` lay outside the box the most), else the nearest.
fn box_side(c: V3, was: V3, ed: &Edge) -> Option<(V3, f32)> {
    if ed.top <= 0.0 {
        return None;
    }
    let l = box_local(c, ed);
    let q = [
        l[0].clamp(-ed.reach, ed.reach),
        l[1].clamp(0.0, ed.top),
        l[2].min(0.0),
    ];
    let off = sub(l, q);
    let world = |v: V3| {
        add(
            add(scale(ed.dir, v[0]), scale(ed.up, v[1])),
            [0.0, 0.0, v[2]],
        )
    };
    let d = length(off);
    if d > 1e-6 {
        return Some((world(scale(off, 1.0 / d)), d));
    }
    // Inside: each side's way out (its outward normal in the box's frame, the distance).
    let outs = [
        ([1.0, 0.0, 0.0], ed.reach - l[0]),
        ([-1.0, 0.0, 0.0], ed.reach + l[0]),
        ([0.0, 1.0, 0.0], ed.top - l[1]),
        ([0.0, -1.0, 0.0], l[1]),
        ([0.0, 0.0, 1.0], -l[2]),
    ];
    let w = box_local(was, ed);
    let came = [
        w[0] - ed.reach,
        -ed.reach - w[0],
        w[1] - ed.top,
        -w[1],
        w[2],
    ];
    let pick = match came
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > 0.0)
        .max_by(|a, b| a.1.total_cmp(b.1))
    {
        Some((i, _)) => i,
        None => outs
            .iter()
            .enumerate()
            .min_by(|a, b| a.1 .1.total_cmp(&b.1 .1))
            .map_or(4, |(i, _)| i),
    };
    let (n, gap) = outs[pick];
    Some((world(n), -gap))
}

/// Whether the wall `w` (a body's own, its top at `top`) is a box edge's face (`edges`) where
/// the point `c` lies along it: the box's contacts hold every body against it ([`box_side`]),
/// so the body's own plane is not used twice; past the box's ends the plane is the body's.
fn boxed_wall(w: &Plane, top: f32, edges: &[Edge], c: V3) -> bool {
    edges.iter().any(|ed| {
        ed.top > 0.0
            && dot(w.n, scale(ed.up, -1.0)) > 0.95
            && dot(sub(ed.p, w.p), w.n).abs() < 0.04
            && (ed.p[2] - top).abs() < 0.03
            && dot(sub(c, ed.p), ed.dir).abs() <= ed.reach
    })
}

/// Body `i`'s floor: its own (`floors`, when given for every body), else the frame's flat one
/// (`surfaces[0]`).
fn floor_of(surfaces: &[Plane], floors: &[Plane], i: usize) -> Plane {
    match floors.get(i) {
        Some(f) if floors.len() > i => *f,
        _ => surfaces[0],
    }
}

/// Body `i`'s floor under its end `end`: its own end's ([`Tracking::end_floors`]), else the
/// body's ([`floor_of`]).
fn end_floor(surfaces: &[Plane], floors: (&[Plane], &[[Plane; 2]]), i: usize, end: usize) -> Plane {
    match floors.1.get(i) {
        Some(ends) => ends[end],
        None => floor_of(surfaces, floors.0, i),
    }
}

fn floor_inertia(inertia: M3, m: f32) -> M3 {
    let least = 0.4 * m * MIN_RADIUS * MIN_RADIUS;
    let mut out = inertia;
    for i in 0..3 {
        if out[i * 4] < least {
            out[i * 4] = least;
        }
    }
    out
}

/// A 3x3 matrix's inverse (a symmetric positive one here).
fn invert(m: &M3) -> M3 {
    let [a, b, c, d, e, f, g, h, i] = *m;
    let co = [
        e * i - f * h,
        c * h - b * i,
        b * f - c * e,
        f * g - d * i,
        a * i - c * g,
        c * d - a * f,
        d * h - e * g,
        b * g - a * h,
        a * e - b * d,
    ];
    let det = a * co[0] + b * co[3] + c * co[6];
    if det.abs() < 1e-12 {
        return [0.0; 9];
    }
    co.map(|x| x / det)
}

/// The lowest point of any capsule (world, metres).
pub fn lowest_point(m: &Model, states: &[State]) -> f32 {
    m.bodies
        .iter()
        .zip(states)
        .flat_map(|(b, s)| {
            b.ends
                .map(|e| add(s.x, qrot(s.q, sub(e, b.com)))[2] - b.radius)
        })
        .fold(f32::INFINITY, f32::min)
}

/// The turn (radians) from rotation `b` to `a`.
pub fn angle(a: Q, b: Q) -> f32 {
    length(qlog(qmul(a, qconj(b))))
}

/// The rotation vector of `w` as a quaternion (re-exported for the controller).
pub fn turn_of(w: V3) -> Q {
    qexp(w)
}

/// How far a body's ends may lie from its centre, as a share of its radius, for it to be pushed out
/// of a surface as a ball ([`is_sphere`]).
pub const SPHERE_LIKE: f32 = 0.25;

/// A sphere-like body's sliding friction as a share of the others' (and no static grip) while it
/// slides faster than [`SPHERE_SKID`] (m/s): a head that hits the ground skids on with the body
/// (held there, the neck swung it round it: a whip); slower, it grips as the rest (lying on a
/// slope, a head that slid let the body slide 38 u).
pub const SPHERE_FRICTION: f32 = 0.3;
pub const SPHERE_SKID: f32 = 0.5;

/// Whether a body is sphere-like: both its ends within [`SPHERE_LIKE`] of its radius from its
/// centre (the head: ends 0.016 from its centre, radius 0.111).
fn is_sphere(def: &BodyDef) -> bool {
    def.ends
        .iter()
        .all(|e| length(sub(*e, def.com)) < SPHERE_LIKE * def.radius)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bind() -> (Model, Vec<State>, Vec<(M3, V3)>) {
        let sk = Skeleton::fnv();
        let m = Model::from_skeleton(&sk);
        let w = sk.bind_worlds();
        let nodes: Vec<(M3, V3)> = m
            .bodies
            .iter()
            .map(|b| {
                let x = w[sk.node(&b.node).unwrap()];
                (x.r, x.t)
            })
            .collect();
        let s = m.states_from_nodes(&nodes);
        (m, s, nodes)
    }

    /// The model is FNV's: 18 bodies of 81 kg, 17 joints, the root NonAccum; its bind pose read
    /// back as the nodes it came from; the lowest capsule on the floor (within 2 units of 0).
    #[test]
    fn the_model_is_fnvs_ragdoll() {
        let (m, s, nodes) = bind();
        assert_eq!(m.bodies.len(), 18);
        assert_eq!(m.joints.len(), 17);
        assert!((m.mass - 81.0).abs() < 1e-3);
        assert_eq!(m.bodies[m.root].node, "Bip01 NonAccum");
        for ((r, t), (r0, t0)) in m.node_worlds(&s).iter().zip(&nodes) {
            assert!(length(sub(*t, *t0)) < 1e-3);
            assert!(crate::np_skeleton::angle_between(r, r0) < 1e-3);
        }
        let lowest = m
            .bodies
            .iter()
            .zip(&s)
            .flat_map(|(b, st)| {
                b.ends
                    .map(|e| add(st.x, qrot(st.q, sub(e, b.com)))[2] - b.radius)
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            lowest.abs() * UNITS < 2.0,
            "the lowest point {:.2} units",
            lowest * UNITS
        );
    }

    /// Driven to its bind pose, held up at its root and standing on the floor, the body stays
    /// put: after 2 s no node more than 1 unit or 1.5 degrees off, no joint apart.
    #[test]
    fn the_driven_body_holds_its_pose() {
        let (m, mut s, nodes) = bind();
        let targets = m.relative(&s);
        let drives: Vec<Drive> = targets
            .iter()
            .map(|&t| Drive {
                target: t,
                kp: 400.0,
                kd: 40.0,
                hold: 1.0,
            })
            .collect();
        let r = m.root;
        let support = Support {
            body: r,
            x: s[r].x,
            v: [0.0; 3],
            q: s[r].q,
            kp: 20000.0,
            kd: 1500.0,
            max_force: 4000.0,
            kp_h: 20000.0,
            kd_h: 1500.0,
            max_force_h: 4000.0,
            kp_turn: 2000.0,
            kd_turn: 150.0,
            max_torque: 1000.0,
        };
        let floor = lowest_point(&m, &s);
        let mut rep = StepReport::default();
        for _ in 0..120 {
            rep = m.step(&mut s, &drives, &[support], floor, 1.0 / 60.0);
        }
        for ((r1, t1), (r0, t0)) in m.node_worlds(&s).iter().zip(&nodes) {
            assert!(length(sub(*t1, *t0)) < 1.0, "{:?} vs {:?}", t1, t0);
            // The knees take a degree as the support spring gives under the body's weight.
            assert!(crate::np_skeleton::angle_between(r1, r0).to_degrees() < 1.5);
        }
        assert!(
            rep.gap_most * UNITS < 0.05,
            "a joint {:.3} units apart",
            rep.gap_most * UNITS
        );
        assert!(rep
            .contacts
            .iter()
            .any(|c| m.bodies[c.0].node.contains("Foot")));
    }

    /// Undriven and unsupported, the body falls onto the floor and stays above it, its joints
    /// together and inside their limits; nothing flies.
    #[test]
    fn a_limp_body_falls_onto_the_floor() {
        let (m, mut s, _) = bind();
        let targets = m.relative(&s);
        let drives: Vec<Drive> = targets
            .iter()
            .map(|&t| Drive {
                target: t,
                kp: 0.0,
                kd: 0.0,
                hold: 0.0,
            })
            .collect();
        let mut rep = StepReport::default();
        for _ in 0..180 {
            rep = m.step(&mut s, &drives, &[], 0.0, 1.0 / 60.0);
        }
        let lowest = m
            .bodies
            .iter()
            .zip(&s)
            .flat_map(|(b, st)| {
                b.ends
                    .map(|e| add(st.x, qrot(st.q, sub(e, b.com)))[2] - b.radius)
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            lowest * UNITS > -1.0,
            "{:.2} units under the floor",
            lowest * UNITS
        );
        assert!(
            rep.gap_most * UNITS < 0.5,
            "a joint {:.2} apart",
            rep.gap_most * UNITS
        );
        assert!(
            rep.limit_most.to_degrees() < 2.0,
            "{:.1} degrees past a limit",
            rep.limit_most.to_degrees()
        );
        let (com, v) = m.centre_of_mass(&s);
        assert!(
            com[2] * UNITS < 25.0,
            "it did not fall: {:.1}",
            com[2] * UNITS
        );
        assert!(length(v) < 1.0, "still moving {:.2} m/s", length(v));
    }

    /// A joint driven past its data limit (a knee straightened 20 degrees past its 87.3) stops at
    /// the limit when the animation does not go there; the impulse's momentum is the body's.
    #[test]
    fn limits_hold_and_momentum_is_kept() {
        let (m, s0, _) = bind();
        let knee = m
            .joints
            .iter()
            .position(|j| m.bodies[j.child].node == "Bip01 L Calf")
            .unwrap();
        let mut s = s0.clone();
        let mut targets = m.relative(&s);
        let drives0: Vec<Drive> = targets
            .iter()
            .map(|&t| Drive {
                target: t,
                kp: 0.0,
                kd: 0.0,
                hold: 0.0,
            })
            .collect();
        // The impulse's momentum: free bodies, no gravity's effect over one substep-free check.
        let j = [10.0, 0.0, 0.0];
        let c = m.body("Bip01 Spine2").unwrap();
        let at = s[c].x;
        m.impulse(&mut s, c, at, j);
        let (_, v) = m.centre_of_mass(&s);
        assert!((v[0] * m.mass - 10.0).abs() < 1e-3);
        // A knee drive past its limit, the target itself inside: held at 87.3 + nothing.
        let mut s = s0;
        let Limit::Hinge { axle, .. } = m.joints[knee].limit else {
            panic!()
        };
        let straighter = qmul(targets[knee], qexp(scale(axle[0], 0.0)));
        targets[knee] = straighter;
        let mut drives = drives0;
        drives[knee] = Drive {
            target: targets[knee],
            kp: 400.0,
            kd: 40.0,
            hold: 0.0,
        };
        // Push the shin hard forward.
        let calf = m.joints[knee].child;
        for _ in 0..30 {
            let w = qrot(s[calf].q, axle[0]);
            s[calf].w = add(s[calf].w, scale(w, 3.0));
            m.step(&mut s, &drives, &[], -10.0, 1.0 / 60.0);
            let rel = m.relative(&s)[knee];
            let Measure::Hinge { angle } = m.measure(knee, rel) else {
                panic!()
            };
            assert!(
                angle.to_degrees() < 87.3 + 3.0 + 1.0 && angle.to_degrees() > -50.0 - 1.0,
                "{}",
                angle.to_degrees()
            );
        }
    }
}

#[cfg(test)]
mod friction_tests {
    use super::*;

    /// Laid on its back and sliding at 3 m/s, limp, the body stops by the floor's sliding
    /// friction in about v² / (2 μ g) (0.57 m at μ 0.8; 0.4 to 0.9 m for the turning and settling
    /// limbs) and stays: Coulomb's friction of each contact's own push, no bounce.
    #[test]
    fn a_sliding_body_stops_by_friction() {
        let m = Model::fnv();
        let (_, nodes) = m.bind();
        let mut st = m.states_from_nodes(&nodes);
        let q = qexp([-std::f32::consts::FRAC_PI_2, 0.0, 0.0]);
        for s in st.iter_mut() {
            s.x = qrot(q, s.x);
            s.q = qmul(q, s.q);
        }
        let low = lowest_point(&m, &st);
        for s in st.iter_mut() {
            s.x[2] -= low;
            s.v = [0.0, -3.0, 0.0];
        }
        let x0 = m.centre_of_mass(&st).0;
        let drives: Vec<Drive> = m
            .relative(&st)
            .iter()
            .map(|&t| Drive {
                target: t,
                kp: 0.0,
                kd: 0.0,
                hold: 0.0,
            })
            .collect();
        let mut high = 0.0f32;
        for _ in 0..120 {
            m.step(&mut st, &drives, &[], 0.0, 1.0 / 60.0);
            high = high.max(m.centre_of_mass(&st).0[2] - x0[2]);
        }
        let (c, v) = m.centre_of_mass(&st);
        let slid = length(sub(c, x0));
        eprintln!(
            "RESULT slid {slid:.2} m, speed at 2 s {:.3} m/s, rose {high:.3} m",
            length(v)
        );
        assert!((0.4..0.9).contains(&slid), "{slid}");
        assert!(length(v) < 0.05 && high < 0.05);
    }
}
