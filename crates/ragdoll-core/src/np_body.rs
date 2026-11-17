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
use crate::np_skeleton::{Limit, Measure, Skeleton, HAVOK_UNITS};

/// Game units to a metre.
pub const UNITS: f32 = crate::UNITS_PER_METRE;
/// Gravity (m/s²).
pub const GRAVITY: f32 = 9.81;
/// Substeps a frame.
pub const SUBSTEPS: usize = 16;
/// How far past the animated pose's own value a limit is widened (radians, 3 degrees).
pub const LIMIT_MARGIN: f32 = 0.052;
/// The floor's friction (static, dynamic).
pub const FRICTION: (f32, f32) = (1.0, 0.8);
/// The longest frame stepped (s): a hitch is stepped as this.
pub const MAX_FRAME: f32 = 0.05;

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
}

/// What holds the root body up this frame: a spring and damper toward a pose (world, metres), each
/// at most `max_force` (N) and `max_torque` (N·m).
#[derive(Clone, Copy, Debug)]
pub struct Support {
    pub x: V3,
    pub q: Q,
    pub kp: f32,
    pub kd: f32,
    pub kp_turn: f32,
    pub kd_turn: f32,
    pub max_force: f32,
    pub max_torque: f32,
}

/// What a frame's step found.
#[derive(Clone, Debug, Default)]
pub struct StepReport {
    /// Bodies touching the floor at the frame's end (with the contact's world point, metres).
    pub contacts: Vec<(usize, V3)>,
    /// The most a joint lay outside its widened limit before the frame's last projection (rad).
    pub limit_most: f32,
    /// The largest joint gap at the end (m).
    pub gap_most: f32,
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
        model
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
    /// held by `support`, standing on the floor at height `floor` (metres).
    pub fn step(
        &self,
        states: &mut [State],
        drives: &[Drive],
        support: Option<&Support>,
        floor: f32,
        dt: f32,
    ) -> StepReport {
        self.step_tracked(states, drives, support, None, floor, dt)
    }

    /// [`Model::step`] with each body's turn tracked to its animation's (`tracking`).
    pub fn step_tracked(
        &self,
        states: &mut [State],
        drives: &[Drive],
        support: Option<&Support>,
        tracking: Option<&Tracking>,
        floor: f32,
        dt: f32,
    ) -> StepReport {
        let mut report = StepReport::default();
        let dt = dt.clamp(0.0, MAX_FRAME);
        if dt <= 0.0 || states.len() != self.bodies.len() {
            return report;
        }
        let h = dt / SUBSTEPS as f32;
        // The limits widened by the animated pose's own values.
        let widened: Vec<Limit> = self
            .joints
            .iter()
            .enumerate()
            .map(|(j, def)| {
                let target = drives.get(j).map_or(Q_ID, |d| d.target);
                widen(def.limit, self.measure(j, target), axle_apart(def, target))
            })
            .collect();
        for sub_i in 0..SUBSTEPS {
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
                let out = self.limit(states, def, &widened[j]);
                if sub_i == SUBSTEPS - 1 {
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
            if let Some(sp) = support {
                self.support(states, sp, h);
            }
            self.floor(states, &prev, floor);
            // Velocities from the moves.
            for (s, p) in states.iter_mut().zip(&prev) {
                s.v = scale(sub(s.x, p.x), 1.0 / h);
                let dq = qmul(s.q, qconj(p.q));
                let w = scale([dq[1], dq[2], dq[3]], 2.0 / h);
                s.w = if dq[0] < 0.0 { scale(w, -1.0) } else { w };
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
                    let k = 1.0 - (-rate * h).exp();
                    st.w = add(st.w, scale(sub(aw, st.w), k));
                }
            }
            // The support's damping, the floor's sliding friction.
            if let Some(sp) = support {
                let r = self.root;
                self.damp_turn(states, r, None, sp.kd_turn, h);
                let k = (sp.kd * self.bodies[r].inv_mass * h).min(1.0);
                states[r].v = scale(states[r].v, 1.0 - k);
            }
            self.friction(states, floor, h);
        }
        for (i, def) in self.bodies.iter().enumerate() {
            for e in def.ends {
                let p = add(states[i].x, qrot(states[i].q, sub(e, def.com)));
                if p[2] - def.radius <= floor + 0.005 {
                    report.contacts.push((i, [p[0], p[1], floor]));
                }
            }
        }
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
    fn limit(&self, states: &mut [State], def: &JointDef, lim: &Limit) -> f32 {
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
                let phi = dot(ta, tb).clamp(-1.0, 1.0).acos();
                if phi > cone {
                    if let Some(n) = unit(cross(ta, tb)) {
                        corrections.push(scale(n, phi - cone));
                        most = most.max(phi - cone);
                    }
                }
                let s = dot(ta, pl_b).clamp(-1.0, 1.0).asin();
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
                    let tau = dot(cross(pb, pa), axis).atan2(dot(pb, pa));
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
                let apart = dot(aa, ab).clamp(-1.0, 1.0).acos();
                if apart > apart_most {
                    if let Some(n) = unit(cross(aa, ab)) {
                        corrections.push(scale(n, apart - apart_most));
                        most = most.max(apart - apart_most);
                    }
                }
                let (pa, pb) = (apply_m(&ra, perp1[0]), apply_m(&rb, perp1[1]));
                let theta = dot(cross(pb, pa), aa).atan2(dot(pb, pa));
                if theta > angles.1 {
                    corrections.push(scale(aa, -(theta - angles.1)));
                    most = most.max(theta - angles.1);
                } else if theta < angles.0 {
                    corrections.push(scale(aa, angles.0 - theta));
                    most = most.max(angles.0 - theta);
                }
            }
        }
        for c in corrections {
            self.turn(states, def.child, Some(def.parent), c, 0.0, 1.0);
        }
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

    /// The root held toward the support's pose: a capped spring.
    fn support(&self, states: &mut [State], sp: &Support, h: f32) {
        let r = self.root;
        let d = sub(sp.x, states[r].x);
        let c = length(d);
        if c > 1e-7 && sp.kp > 0.0 {
            let n = scale(d, 1.0 / c);
            let w = self.bodies[r].inv_mass;
            let alpha = 1.0 / (sp.kp * h * h);
            let lambda = (c / (w + alpha)).min(sp.max_force * h * h);
            states[r].x = add(states[r].x, scale(n, lambda * w));
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

    /// The capsules kept above the floor; a contact that has not slid past the static friction is
    /// held where it was.
    fn floor(&self, states: &mut [State], prev: &[State], floor: f32) {
        for (i, def) in self.bodies.iter().enumerate() {
            for e in def.ends {
                let local = sub(e, def.com);
                let centre = add(states[i].x, qrot(states[i].q, local));
                let depth = floor - (centre[2] - def.radius);
                if depth <= 0.0 {
                    continue;
                }
                let n = [0.0, 0.0, 1.0];
                let p = [centre[0], centre[1], centre[2] - def.radius];
                let r = sub(p, states[i].x);
                let w = self.point_weight(states, i, r, n);
                let lambda = depth / w;
                self.move_at(states, i, r, scale(n, lambda));
                // Static friction: the contact point's slide this substep taken back while it is
                // within the friction cone of the push out.
                let now = add(states[i].x, qrot(states[i].q, local));
                let was = add(prev[i].x, qrot(prev[i].q, local));
                let slide = sub(now, was);
                let slide = [slide[0], slide[1], 0.0];
                let s = length(slide);
                if s > 1e-7 && s < FRICTION.0 * depth {
                    let t = scale(slide, -1.0 / s);
                    let r = sub([now[0], now[1], now[2] - def.radius], states[i].x);
                    let w = self.point_weight(states, i, r, t);
                    self.move_at(states, i, r, scale(t, s / w));
                }
            }
        }
    }

    /// The floor's sliding friction on the velocities of the bodies touching it.
    fn friction(&self, states: &mut [State], floor: f32, h: f32) {
        for (i, def) in self.bodies.iter().enumerate() {
            for e in def.ends {
                let local = sub(e, def.com);
                let centre = add(states[i].x, qrot(states[i].q, local));
                if centre[2] - def.radius > floor + 0.002 {
                    continue;
                }
                let r = sub([centre[0], centre[1], centre[2] - def.radius], states[i].x);
                let v = add(states[i].v, cross(states[i].w, r));
                let vt = [v[0], v[1], 0.0];
                let s = length(vt);
                if s < 1e-6 {
                    continue;
                }
                // At most the friction of the body's weight over the substep.
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

/// A hinge's axles' angle apart at the relative rotation `rel` (0 for a ragdoll joint).
fn axle_apart(def: &JointDef, rel: Q) -> f32 {
    match def.limit {
        Limit::Hinge { axle, .. } => {
            let aa = qrot(rel, axle[0]);
            dot(aa, axle[1]).clamp(-1.0, 1.0).acos()
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
            x: s[r].x,
            q: s[r].q,
            kp: 20000.0,
            kd: 1500.0,
            kp_turn: 2000.0,
            kd_turn: 150.0,
            max_force: 4000.0,
            max_torque: 1000.0,
        };
        let floor = lowest_point(&m, &s);
        let mut rep = StepReport::default();
        for _ in 0..120 {
            rep = m.step(&mut s, &drives, Some(&support), floor, 1.0 / 60.0);
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
            rep = m.step(&mut s, &drives, None, 0.0, 1.0 / 60.0);
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
            m.step(&mut s, &drives, None, -10.0, 1.0 / 60.0);
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
