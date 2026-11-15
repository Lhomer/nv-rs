//! The active ragdoll's body (Patrick, 2026-11-15: toward Euphoria): for a second or two after a
//! hit the struck body is a physical articulated body, every joint a PD motor tracking the
//! animation's pose (soft at first, its strength ramping back), the hit an impulse on the struck
//! part that the joints carry up and down the chain over time, its balance from the motors, a
//! support spring at the pelvis following the capture-point steps ([`crate::balance`]) and the
//! feet pinned to where they stand or swing. Then it blends back to the animation, eased.
//!
//! **The simulation.** Each bone is a rigid capsule (its mass a share of the body's, [`share`];
//! its inertia a capsule's) with its joint at one end. Small steps ([`SUBSTEPS`] a frame): the
//! motors, the support and the pins as forces and torques (explicit, stable at these steps), the
//! bodies moved, then the joints held together by positional constraints (XPBD, no compliance) and
//! the velocities taken from the moves, so every velocity is continuous. Units: metres, seconds,
//! kilograms; rotations as quaternions `[w, x, y, z]` (world space).
//!
//! **Its use** (the host's `crate::active_ragdoll` and `tools/ragdoll-lab`): [`Body::from_pose`] at
//! the hit (the shown pose and its motion, so nothing jumps), [`Body::hit`], each frame
//! [`Body::step`] with the targets ([`Targets`]: the animation's pose with the steps' legs, a
//! brace's hands), [`Body::pose`] for the bones, blended with the animation by [`weight`].

use crate::flinch::{add, cross, dot, length, scale, sub, unit, Role, M3, V3};
use crate::tuning;

/// Small steps a frame.
pub const SUBSTEPS: usize = 8;
/// Gravity (m/s2, down the z axis).
pub const GRAVITY: f32 = 9.81;
/// How long the body stays physical after its last hit (s), how long it then blends back to the
/// animation (s).
pub const ACTIVE_FOR: f32 = 1.4;
pub const BLEND_BACK: f32 = 0.8;
/// The motors: their strength right after a hit (a share of full), how long it takes to ramp back
/// (s), their frequency against each role's spring frequency, their damping ratio (soft and well
/// damped).
pub const DRIVE_SOFT: f32 = 0.45;
pub const DRIVE_RAMP: f32 = 0.7;
pub const DRIVE_SCALE: f32 = 2.0;
pub const DRIVE_ZETA: f32 = 1.0;
/// The pelvis's support (Hz, damping ratio) and the feet's pins (Hz, damping ratio).
pub const SUPPORT_HZ: f32 = 1.4;
pub const SUPPORT_ZETA: f32 = 1.0;
pub const FOOT_HZ: f32 = 4.0;
pub const FOOT_ZETA: f32 = 1.0;
/// A joint turns at most this far from its target (a share of a bent limb's freedom, rad).
pub const MAX_BEND: f32 = 1.2;
/// ... and at most this many times its role's limit (`crate::flinch::Role::limit`).
pub const BEND_ROOM: f32 = 1.5;
/// A hit's momentum goes in over this long (s), eased in and out (no jump in any velocity or
/// acceleration), while the joints carry it along the chain.
pub const HIT_TIME: f32 = 0.06;
/// A hit's push reaches each bone after its distance from the impact at this speed (m/s): it
/// travels up and down the body.
pub const HIT_WAVE: f32 = 5.0;
/// A joint turns against its parent at most this many times its role's spin
/// (`crate::flinch::Role::spin`).
pub const SPIN_ROOM: f32 = 1.3;
/// An arm's joint at most its role's spin (the human arms of 2026-11-15: 4 to 6 rad/s), the speed
/// saturating smoothly from [`SPIN_KNEE`] of it.
pub const ARM_SPIN_ROOM: f32 = 1.1;
/// A bone's speed (m/s) and spin (rad/s) at most, and how far from the pelvis any bone may be (m)
/// before the body is not believed ([`Body::check`]).
pub const MAX_SPEED: f32 = 12.0;
pub const MAX_SPIN: f32 = 30.0;
pub const MAX_SPREAD: f32 = 2.5;
pub const SPIN_KNEE: f32 = 0.6;
/// The elbows and the knees are one-way hinges ([`hinge_limit`]): how far each flexes (rad: an
/// elbow about 145 degrees, a knee about 140), how far past straight it may go (a hair: none), how
/// far its child may lean out of the hinge's plane (rad: an elbow's carrying angle, a knee's
/// play), and how bent the animation must be for its bend to say the hinge's axis (rad).
pub const ELBOW_FLEX: f32 = 2.5;
pub const KNEE_FLEX: f32 = 2.4;
pub const HYPEREXTEND: f32 = 0.03;
pub const ELBOW_LEAN: f32 = 0.3;
pub const KNEE_LEAN: f32 = 0.15;
pub const AXIS_BEND: f32 = 0.2;
/// How much firmer a hinge's motor is near and past its stop (as a negative dip).
pub const HINGE_FIRM: f32 = 4.0;
/// How near its stop (rad) a hinge's motor is made firmer.
pub const HINGE_NEAR: f32 = 0.08;
/// The share of the way back into its range a hinge past it is turned each small step.
pub const HINGE_SHARE: f32 = 0.05;

/// Each Bip01 bone's share of the body's mass (`crate::flinch::BONES`' order; segment fractions).
pub const SHARES: [f32; 20] = [
    0.142, 0.0695, 0.0695, 0.206, 0.010, 0.071, 0.005, 0.005, 0.028, 0.028, 0.016, 0.016, 0.100,
    0.100, 0.0465, 0.0465, 0.0145, 0.0145, 0.006, 0.006,
];

/// The share of the body's mass of the bone named `name` (one of `crate::flinch::BONES`).
pub fn share(name: &str) -> f32 {
    crate::flinch::BONES
        .iter()
        .position(|b| b.0.eq_ignore_ascii_case(name))
        .map_or(0.01, |i| SHARES[i])
}

// --- Quaternions. ----------------------------------------------------------------------------------

pub type Q = [f32; 4];
pub const Q_ID: Q = [1.0, 0.0, 0.0, 0.0];

pub fn qmul(a: Q, b: Q) -> Q {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}
pub fn qconj(a: Q) -> Q {
    [a[0], -a[1], -a[2], -a[3]]
}
pub fn qnorm(a: Q) -> Q {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2] + a[3] * a[3]).sqrt();
    if !(l.is_finite() && l > 1e-9) {
        return Q_ID;
    }
    [a[0] / l, a[1] / l, a[2] / l, a[3] / l]
}
pub fn qrot(q: Q, v: V3) -> V3 {
    let p = qmul(qmul(q, [0.0, v[0], v[1], v[2]]), qconj(q));
    [p[1], p[2], p[3]]
}
/// The rotation of the rotation vector `w`.
pub fn qexp(w: V3) -> Q {
    let a = length(w);
    if !(a.is_finite() && a > 1e-9) {
        return Q_ID;
    }
    let (s, c) = (a / 2.0).sin_cos();
    let k = s / a;
    [c, w[0] * k, w[1] * k, w[2] * k]
}
/// The rotation vector of `q` (the shorter way round).
pub fn qlog(q: Q) -> V3 {
    let q = if q[0] < 0.0 {
        [-q[0], -q[1], -q[2], -q[3]]
    } else {
        q
    };
    let v = [q[1], q[2], q[3]];
    let s = length(v);
    if s < 1e-9 {
        return scale(v, 2.0);
    }
    let a = 2.0 * s.atan2(q[0]);
    scale(v, a / s)
}
pub fn qslerp(a: Q, b: Q, t: f32) -> Q {
    let d = qmul(qconj(a), b);
    qnorm(qmul(a, qexp(scale(qlog(d), t.clamp(0.0, 1.0)))))
}
/// A rotation matrix (row-major, as `NiMatrix3`) as a quaternion.
pub fn qfrom_m(m: &M3) -> Q {
    let tr = m[0] + m[4] + m[8];
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [
            s / 4.0,
            (m[7] - m[5]) / s,
            (m[2] - m[6]) / s,
            (m[3] - m[1]) / s,
        ]
    } else if m[0] > m[4] && m[0] > m[8] {
        let s = (1.0 + m[0] - m[4] - m[8]).sqrt() * 2.0;
        [
            (m[7] - m[5]) / s,
            s / 4.0,
            (m[1] + m[3]) / s,
            (m[2] + m[6]) / s,
        ]
    } else if m[4] > m[8] {
        let s = (1.0 + m[4] - m[0] - m[8]).sqrt() * 2.0;
        [
            (m[2] - m[6]) / s,
            (m[1] + m[3]) / s,
            s / 4.0,
            (m[5] + m[7]) / s,
        ]
    } else {
        let s = (1.0 + m[8] - m[0] - m[4]).sqrt() * 2.0;
        [
            (m[3] - m[1]) / s,
            (m[2] + m[6]) / s,
            (m[5] + m[7]) / s,
            s / 4.0,
        ]
    };
    qnorm(q)
}
pub fn m_from_q(q: Q) -> M3 {
    let [w, x, y, z] = q;
    [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y - w * z),
        2.0 * (x * z + w * y),
        2.0 * (x * y + w * z),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z - w * x),
        2.0 * (x * z - w * y),
        2.0 * (y * z + w * x),
        1.0 - 2.0 * (x * x + y * y),
    ]
}

fn mv(m: &M3, v: V3) -> V3 {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

// --- Easing. ---------------------------------------------------------------------------------------

/// `6t^5 - 15t^4 + 10t^3`: from 0 to 1 with no jump in speed or acceleration at either end.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// The physics' weight in the shown pose `since` seconds after the last hit (1 while active, then
/// eased down to 0 over [`BLEND_BACK`]).
pub fn weight(since: f32) -> f32 {
    let t = tuning::get();
    1.0 - ease((since - t.active_for) / t.blend_back.max(0.05))
}

/// The motors' strength `since` seconds after the last hit (soft, eased back to full).
pub fn strength(since: f32) -> f32 {
    let t = tuning::get();
    t.drive_soft + (1.0 - t.drive_soft) * ease(since / t.drive_ramp.max(0.05))
}

// --- The body. -------------------------------------------------------------------------------------

/// A bone as the body knows it: its parent bone, role, mass (kg), its segment from its joint
/// (bone-local, m), its radius (m), where its joint sits in its parent (the parent's local frame,
/// from the parent's joint, m), its inverse inertia about its centre (bone-local).
#[derive(Clone, Debug)]
pub struct Segment {
    pub parent: Option<usize>,
    pub role: Role,
    pub mass: f32,
    pub seg: V3,
    pub radius: f32,
    pub anchor: V3,
    pub inv_inertia: M3,
}

/// A bone's motion: its centre (m), its rotation, its velocity (m/s) and spin (rad/s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub com: V3,
    pub q: Q,
    pub v: V3,
    pub w: V3,
}

/// What the body tracks this frame: each bone's world rotation; the pelvis's joint (m); the feet's
/// targets (a foot bone and its joint's target, m), the hands' (a hand bone and its target).
#[derive(Clone, Debug, Default)]
pub struct Targets {
    pub rot: Vec<Q>,
    pub pelvis: V3,
    pub feet: Vec<(usize, V3)>,
    pub hands: Vec<(usize, V3)>,
    /// The support's share (1 standing; less as it goes over).
    pub support: f32,
    /// Per bone (empty: none): how much its motor tracks its target's world rotation instead of
    /// its rotation relative to its parent (0 to 1): an armed body's arms keep their aim while the
    /// torso under them flinches.
    pub world: Vec<f32>,
    /// Per bone (empty: `rot`): the plain animation's world rotations, before any reach or step
    /// (an elbow's or a knee's hinge axis is taken from them, never from a reach that might be
    /// wrong).
    pub anim: Vec<Q>,
    /// The way the actor faces (world, flat; zero: not known): a straight limb's hinge axis.
    pub forward: V3,
}

/// The physical body (module documentation).
#[derive(Clone, Debug)]
pub struct Body {
    pub segments: Vec<Segment>,
    pub state: Vec<State>,
    pub pelvis: usize,
    pub mass: f32,
    /// Per bone: how much its motor is weakened now (a hit's dip near it, fading).
    pub dip: Vec<f32>,
    /// The hits going in: bone, momentum (N s, world), the point (bone-local, m), time in (s).
    pub pushes: Vec<(usize, V3, V3, f32)>,
    /// Per bone: an elbow's or a knee's hinge axis, in its parent's frame (the way it flexes: a
    /// turn about it by a positive angle bends it), once known ([`Body::hinge_bend`]).
    pub hinge: Vec<Option<V3>>,
}

/// A joint as given to [`Body::from_pose`]: its bone's parent, role, world rotation, joint and
/// the end of its segment (world, m), radius (m) and mass (kg); and the bone's rotation and joint
/// a frame before (for its motion).
#[derive(Clone, Copy, Debug)]
pub struct Bone {
    pub parent: Option<usize>,
    pub role: Role,
    pub q: Q,
    pub joint: V3,
    pub end: V3,
    pub radius: f32,
    pub mass: f32,
    pub before: Option<(Q, V3)>,
}

fn inv_inertia(mass: f32, seg: V3, radius: f32) -> M3 {
    let len = length(seg).max(0.02);
    let u = unit(seg).unwrap_or([0.0, 0.0, 1.0]);
    let along = (mass * radius * radius / 2.0).max(1e-4);
    let across = (mass * (len * len / 12.0 + radius * radius / 4.0)).max(1e-4);
    let mut m = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            let uu = u[r] * u[c];
            let id = if r == c { 1.0 } else { 0.0 };
            m[r * 3 + c] = (id - uu) / across + uu / along;
        }
    }
    m
}

impl Body {
    /// A body in the pose `bones` (parents before children), moving as it moved over `dt`.
    pub fn from_pose(bones: &[Bone], dt: f32) -> Body {
        let mut segments = Vec::with_capacity(bones.len());
        let mut state = Vec::with_capacity(bones.len());
        let mut pelvis = 0;
        for (i, b) in bones.iter().enumerate() {
            let inv = qconj(b.q);
            let seg = qrot(inv, sub(b.end, b.joint));
            let anchor = match b.parent {
                Some(p) => qrot(qconj(bones[p].q), sub(b.joint, bones[p].joint)),
                None => [0.0; 3],
            };
            if b.parent.is_none() && b.role == Role::Pelvis {
                pelvis = i;
            }
            let com = add(b.joint, scale(sub(b.end, b.joint), 0.5));
            let (v, w) = match b.before {
                Some((q0, j0)) if dt > 1e-4 => {
                    let c0 = add(j0, qrot(q0, scale(seg, 0.5)));
                    (
                        scale(sub(com, c0), 1.0 / dt),
                        scale(qlog(qmul(b.q, qconj(q0))), 1.0 / dt),
                    )
                }
                _ => ([0.0; 3], [0.0; 3]),
            };
            segments.push(Segment {
                parent: b.parent,
                role: b.role,
                mass: b.mass.max(0.05),
                seg,
                radius: b.radius,
                anchor,
                inv_inertia: inv_inertia(b.mass.max(0.05), seg, b.radius),
            });
            state.push(State {
                com,
                q: qnorm(b.q),
                v: if v.iter().all(|x| x.is_finite()) {
                    v
                } else {
                    [0.0; 3]
                },
                w: if w.iter().all(|x| x.is_finite()) {
                    w
                } else {
                    [0.0; 3]
                },
            });
        }
        let mass = segments.iter().map(|s| s.mass).sum();
        let n = segments.len();
        Body {
            segments,
            state,
            pelvis,
            mass,
            dip: vec![0.0; n],
            pushes: Vec::new(),
            hinge: vec![None; n],
        }
    }

    /// An elbow's or a knee's flex range and lean, or `None` for any other joint.
    fn hinge_range(&self, i: usize) -> Option<(f32, f32)> {
        match self.segments[i].role {
            Role::Forearm => Some((ELBOW_FLEX, ELBOW_LEAN)),
            Role::Calf => Some((KNEE_FLEX, KNEE_LEAN)),
            _ => None,
        }
    }

    /// Each hinge's axis from this frame's animation: its bend's axis when it is bent enough to
    /// say it, else (a straight limb) the one that flexes it the way a person's does from the way
    /// the actor faces (an elbow brings the forearm forward, a knee the calf back), else as it
    /// was.
    fn update_hinges(&mut self, targets: &Targets) {
        let anim = if targets.anim.len() == self.state.len() {
            &targets.anim
        } else {
            &targets.rot
        };
        let forward = unit([targets.forward[0], targets.forward[1], 0.0]);
        for i in 0..self.state.len() {
            let (Some(p), Some(_)) = (self.segments[i].parent, self.hinge_range(i)) else {
                continue;
            };
            let (Some(u), Some(f)) = (
                unit(qrot(anim[p], self.segments[p].seg)),
                unit(qrot(anim[i], self.segments[i].seg)),
            ) else {
                continue;
            };
            let bent = cross(u, f);
            let axis = if length(bent) > AXIS_BEND.sin() {
                unit(bent)
            } else {
                forward.and_then(|fw| {
                    let toward = if self.segments[i].role == Role::Calf {
                        scale(fw, -1.0)
                    } else {
                        fw
                    };
                    let a = cross(u, toward);
                    if length(a) > 0.3 {
                        unit(a)
                    } else {
                        None
                    }
                })
            };
            if let Some(a) = axis {
                self.hinge[i] = Some(qrot(qconj(anim[p]), a));
            }
        }
    }

    /// An elbow's or a knee's bend now: its flex (rad, negative: bent backwards) and its lean out
    /// of the hinge's plane (rad), once its axis is known.
    pub fn hinge_bend(&self, i: usize) -> Option<(f32, f32)> {
        let p = self.segments[i].parent?;
        let axis = self.hinge[i]?;
        let rel = qmul(qconj(self.state[p].q), self.state[i].q);
        let u = unit(self.segments[p].seg)?;
        let f = unit(qrot(rel, self.segments[i].seg))?;
        Some(hinge_angles(u, f, axis))
    }

    /// World rotations `rot` (one per bone, parents first) with each elbow and knee held to its
    /// human range ([`hinge_limit`]), or `None` when all already are: what is shown and what the
    /// motors track never asks a joint to bend backwards or past its range.
    pub fn hinged(&self, rot: &[Q]) -> Option<Vec<Q>> {
        if rot.len() != self.state.len() {
            return None;
        }
        let mut out: Option<Vec<Q>> = None;
        for i in 0..self.state.len() {
            let (Some(p), Some((flex, lean)), Some(axis)) =
                (self.segments[i].parent, self.hinge_range(i), self.hinge[i])
            else {
                continue;
            };
            let now = out.as_deref().unwrap_or(rot);
            let rel = qmul(qconj(now[p]), now[i]);
            let (Some(u), Some(f)) = (
                unit(self.segments[p].seg),
                unit(qrot(rel, self.segments[i].seg)),
            ) else {
                continue;
            };
            if let Some(turn) = hinge_limit(u, f, axis, flex, lean) {
                let fixed = qnorm(qmul(now[p], qmul(turn, rel)));
                out.get_or_insert_with(|| rot.to_vec())[i] = fixed;
            }
        }
        out
    }

    /// The pose to show: the targets' world rotations `target` (held to the hinges' ranges,
    /// [`Body::hinged`]) and the body's own, blended by `w` (0 the targets, 1 the body). Each
    /// elbow and knee is blended against its parent, not in the world: two poses within range
    /// blended bone by bone in the world can take the joint between them past straight.
    pub fn shown(&self, target: &[Q], w: f32) -> Vec<Q> {
        let wanted = self.hinged(target).unwrap_or_else(|| target.to_vec());
        if wanted.len() != self.state.len() {
            return wanted;
        }
        let w = w.clamp(0.0, 1.0);
        let mut out: Vec<Q> = Vec::with_capacity(wanted.len());
        for i in 0..wanted.len() {
            let body = self.state[i].q;
            let q = match (self.segments[i].parent, self.hinge_range(i)) {
                (Some(p), Some(_)) if p < i => {
                    let local_target = qmul(qconj(wanted[p]), wanted[i]);
                    let local_body = qmul(qconj(self.state[p].q), body);
                    qnorm(qmul(out[p], qslerp(local_target, local_body, w)))
                }
                _ => qslerp(wanted[i], body, w),
            };
            out.push(q);
        }
        out
    }

    fn hinged_targets(&self, targets: &Targets) -> Option<Targets> {
        let rot = self.hinged(&targets.rot)?;
        let mut t = targets.clone();
        t.rot = rot;
        Some(t)
    }

    /// Each elbow and knee turned back toward its human range when past it (flex only, never
    /// backwards; a little lean), a share of the way each small step ([`HINGE_SHARE`]; the whole
    /// way at once snapped the bone, a jump in its speed), about its joint.
    fn limit_hinges(&mut self) {
        for i in 0..self.state.len() {
            let (Some(p), Some((flex, lean)), Some(axis)) =
                (self.segments[i].parent, self.hinge_range(i), self.hinge[i])
            else {
                continue;
            };
            let qp = self.state[p].q;
            let rel = qmul(qconj(qp), self.state[i].q);
            let (Some(u), Some(f)) = (
                unit(self.segments[p].seg),
                unit(qrot(rel, self.segments[i].seg)),
            ) else {
                continue;
            };
            if let Some(turn) = hinge_limit(u, f, axis, flex, lean) {
                let turn = qexp(scale(qlog(turn), HINGE_SHARE));
                let joint = self.joint(i);
                let q = qnorm(qmul(qp, qmul(turn, rel)));
                self.state[i].q = q;
                let seg = self.segments[i].seg;
                self.state[i].com = add(joint, qrot(q, scale(seg, 0.5)));
            }
        }
    }

    /// Each elbow and knee stopped at its human range: past it (backwards, or flexed past its
    /// stop), the spin of the child against its parent that would take it further is taken out,
    /// shared between the two by their inertia about the hinge (an inelastic stop: no turn is
    /// forced on the positions, which made a spin of its own). The motors' targets (held to the
    /// range) bring it back; what is written is held to it.
    fn stop_hinges(&mut self) {
        for i in 0..self.state.len() {
            let (Some(p), Some((flex, _))) = (self.segments[i].parent, self.hinge_range(i)) else {
                continue;
            };
            let (Some(axis), Some((now, _))) = (self.hinge[i], self.hinge_bend(i)) else {
                continue;
            };
            // Its motor firmer near its stops and past them (back toward its target, held to the
            // range), so it seldom gets there.
            if now < HINGE_NEAR - HYPEREXTEND || now > flex - HINGE_NEAR {
                self.dip[i] = self.dip[i].min(-HINGE_FIRM);
            }
            let past_low = now < -HYPEREXTEND;
            let past_high = now > flex;
            if !(past_low || past_high) {
                continue;
            }
            let Some(n) = unit(qrot(self.state[p].q, axis)) else {
                continue;
            };
            let c = dot(sub(self.state[i].w, self.state[p].w), n);
            if (past_low && c >= 0.0) || (past_high && c <= 0.0) {
                continue;
            }
            let ic = dot(mv(&self.inv_inertia_world(i), n), n).max(0.0);
            let ip = dot(mv(&self.inv_inertia_world(p), n), n).max(0.0);
            let total = ic + ip;
            if total <= 1e-9 {
                continue;
            }
            self.state[i].w = sub(self.state[i].w, scale(n, c * ic / total));
            self.state[p].w = add(self.state[p].w, scale(n, c * ip / total));
        }
    }

    /// A bone's joint now (world, m).
    pub fn joint(&self, i: usize) -> V3 {
        let s = &self.state[i];
        sub(s.com, qrot(s.q, scale(self.segments[i].seg, 0.5)))
    }

    fn inv_inertia_world(&self, i: usize) -> M3 {
        let r = m_from_q(self.state[i].q);
        let l = &self.segments[i].inv_inertia;
        let rt = crate::flinch::transpose(&r);
        crate::flinch::mul(&crate::flinch::mul(&r, l), &rt)
    }

    /// The bone nearest the point `at` (world, m) (its segment's nearest point).
    pub fn nearest(&self, at: V3) -> usize {
        let mut best = (f32::MAX, 0);
        for i in 0..self.state.len() {
            let a = self.joint(i);
            let b = add(a, qrot(self.state[i].q, self.segments[i].seg));
            let ab = sub(b, a);
            let t = (dot(sub(at, a), ab) / dot(ab, ab).max(1e-9)).clamp(0.0, 1.0);
            let d = length(sub(at, add(a, scale(ab, t))));
            if d < best.0 {
                best = (d, i);
            }
        }
        best.1
    }

    /// An impulse `j` (N s, world) at `at` (world, m) on bone `i`.
    pub fn impulse(&mut self, i: usize, j: V3, at: V3) {
        let ii = self.inv_inertia_world(i);
        let s = &mut self.state[i];
        let m = self.segments[i].mass;
        s.v = add(s.v, scale(j, 1.0 / m));
        let r = sub(at, s.com);
        s.w = add(s.w, mv(&ii, cross(r, j)));
    }

    /// Each bone's distance from the point `at` (its segment's nearest point, m).
    pub fn distances(&self, at: V3) -> Vec<f32> {
        (0..self.state.len())
            .map(|i| {
                let a = self.joint(i);
                let b = add(a, qrot(self.state[i].q, self.segments[i].seg));
                let ab = sub(b, a);
                let t = (dot(sub(at, a), ab) / dot(ab, ab).max(1e-9)).clamp(0.0, 1.0);
                length(sub(at, add(a, scale(ab, t))))
            })
            .collect()
    }

    /// A hit: momentum `p` (N s) along `dir` (unit) at `at` (m), shared among the bones by mass,
    /// fading with distance over `spread` (m: the flinch's, wider for a bigger hit), going in over
    /// [`HIT_TIME`]; the motors near it weakened by `dips` (per bone, 0 to 0.9; below 0 stronger).
    /// `takes` (per bone, empty for all 1): how much of its share a bone takes (an aiming body's
    /// arms none: the torso takes it). Answers the struck bone.
    pub fn hit(
        &mut self,
        p: f32,
        dir: V3,
        at: V3,
        spread: f32,
        dips: &[f32],
        takes: &[f32],
    ) -> usize {
        let i = self.nearest(at);
        let d = self.distances(at);
        let w: Vec<f32> = (0..self.state.len())
            .map(|b| {
                self.segments[b].mass
                    * (-d[b] / spread.max(0.05)).exp()
                    * takes.get(b).copied().unwrap_or(1.0)
            })
            .collect();
        let total: f32 = w.iter().sum::<f32>().max(1e-6);
        for (b, wb) in w.iter().enumerate() {
            let share = wb / total;
            if share < 0.005 {
                continue;
            }
            let s = self.state[b];
            // At the impact's height on that bone (its nearest point to the impact).
            let a = self.joint(b);
            let e = add(a, qrot(s.q, self.segments[b].seg));
            let ab = sub(e, a);
            let t = (dot(sub(at, a), ab) / dot(ab, ab).max(1e-9)).clamp(0.0, 1.0);
            let point = if b == i { at } else { add(a, scale(ab, t)) };
            let local = qrot(qconj(s.q), sub(point, s.com));
            let delay = d[b] / tuning::get().hit_wave.max(0.1);
            self.pushes.push((b, scale(dir, p * share), local, -delay));
        }
        for (d, add_) in self.dip.iter_mut().zip(dips) {
            // A dip weakens a motor (up to 0.9); a negative one makes it stronger (an armed body's
            // arms keeping their aim).
            *d = (*d + add_).clamp(-5.0, 0.9);
        }
        i
    }

    /// One frame of `dt` seconds toward `targets`, the motors at `strength` (0 to 1) of full.
    pub fn step(&mut self, targets: &Targets, dt: f32, strength: f32) {
        let t = tuning::get();
        let dt = dt.clamp(0.0, 0.05);
        if dt <= 0.0 || targets.rot.len() != self.state.len() {
            return;
        }
        // Small steps no longer than `1 / (60 x SUBSTEPS)` s however long the frame (a slow frame,
        // a hitch: 50 ms in 8 steps of 6 ms blew an armed body up in the lab).
        let n = ((dt * 60.0 * t.substeps).ceil()).clamp(1.0, 64.0) as usize;
        let h = dt / n as f32;
        let fade = (-dt / t.muscle_back.max(0.02)).exp();
        self.update_hinges(targets);
        let hinged = self.hinged_targets(targets);
        let targets = hinged.as_ref().unwrap_or(targets);
        let gains = self.gains(targets, strength);
        for _ in 0..n {
            // Gravity, and the support's carrying of the body's weight (its share).
            let support = targets.support.clamp(0.0, 1.0);
            for s in &mut self.state {
                s.v[2] -= GRAVITY * h;
            }
            let p = self.pelvis;
            let carry = self.mass * GRAVITY * support * h / self.segments[p].mass;
            self.state[p].v[2] += carry;
            self.carry_limbs(support, h);
            self.push_in(h);
            // Before the move too: a hit's push never moves a limb faster than it may turn.
            self.limit_spin();
            let before: Vec<State> = self.state.clone();
            for s in &mut self.state {
                s.com = add(s.com, scale(s.v, h));
                s.q = qnorm(qmul(qexp(scale(s.w, h)), s.q));
            }
            for _ in 0..2 {
                self.hold_joints();
            }
            self.drives(targets, &gains, h);
            self.pins(targets, &gains, h);
            self.hold_joints();
            self.limit_bends(targets);
            self.limit_hinges();
            self.limit_turns(&before, h);
            for (s, b) in self.state.iter_mut().zip(&before) {
                let v = scale(sub(s.com, b.com), 1.0 / h);
                let w = scale(qlog(qmul(s.q, qconj(b.q))), 1.0 / h);
                if v.iter().chain(w.iter()).all(|x| x.is_finite()) {
                    s.v = v;
                    s.w = w;
                } else {
                    *s = *b;
                    s.v = [0.0; 3];
                    s.w = [0.0; 3];
                }
            }
            self.stop_hinges();
            self.damp(targets, &gains, h);
        }
        for d in &mut self.dip {
            *d *= fade;
        }
    }

    /// Each joint carries the weight of what hangs below it (its gravity moment about the joint,
    /// taken back by an equal and opposite turn on the bone and its parent), times `support`: the
    /// motors only shape the motion, they do not hold the limbs up (soft as they are after a hit).
    fn carry_limbs(&mut self, support: f32, h: f32) {
        if support <= 0.0 {
            return;
        }
        let count = self.state.len();
        // Each bone's subtree: mass and its mass-weighted centre, from the leaves up.
        let mut mass: Vec<f32> = self.segments.iter().map(|s| s.mass).collect();
        let mut moment: Vec<V3> = (0..count)
            .map(|i| scale(self.state[i].com, self.segments[i].mass))
            .collect();
        for i in (0..count).rev() {
            if let Some(p) = self.segments[i].parent {
                mass[p] += mass[i];
                moment[p] = add(moment[p], moment[i]);
            }
        }
        for i in 0..count {
            let Some(p) = self.segments[i].parent else {
                continue;
            };
            let centre = scale(moment[i], 1.0 / mass[i].max(1e-6));
            let lever = sub(centre, self.joint(i));
            // Gravity's turn on the subtree about the joint, taken back.
            let tau = scale(cross(lever, [0.0, 0.0, -GRAVITY * mass[i]]), -support * h);
            let ii = self.inv_inertia_world(i);
            let ip = self.inv_inertia_world(p);
            self.state[i].w = add(self.state[i].w, mv(&ii, tau));
            self.state[p].w = sub(self.state[p].w, mv(&ip, tau));
        }
    }

    /// Each joint's turn against its parent held to its role's speed (an arm's to
    /// [`ARM_SPIN_ROOM`] times its spin, about 4 to 6 rad/s; others' to [`SPIN_ROOM`] times),
    /// smoothly: below [`SPIN_KNEE`] of the bound nothing changes, above it the speed saturates
    /// toward the bound (`tanh`), so a big hit's arms slow into their limit instead of being
    /// clipped. Taken off the child (the lighter end): the parent's motion is not touched.
    fn limit_spin(&mut self) {
        let t = tuning::get();
        for i in 0..self.state.len() {
            let Some(p) = self.segments[i].parent else {
                continue;
            };
            let role = self.segments[i].role;
            let room = if role.is_arm() {
                t.arm_spin_room
            } else {
                t.spin_room
            };
            let most = role.spin() * room;
            let rel = sub(self.state[i].w, self.state[p].w);
            let speed = length(rel);
            let knee = most * SPIN_KNEE;
            if speed > knee && speed.is_finite() && most > knee {
                let allowed = knee + (most - knee) * ((speed - knee) / (most - knee)).tanh();
                let off = scale(rel, 1.0 - allowed / speed);
                self.state[i].w = sub(self.state[i].w, off);
            }
        }
    }

    /// The arms' bound on what the step moved (its constraints and motors move the bones too):
    /// each arm joint's turn against its parent over the small step `h` eased back to its allowed
    /// speed, the bone turned about its joint (parents first, so each child rides its parent's
    /// final pose).
    fn limit_turns(&mut self, before: &[State], h: f32) {
        let t = tuning::get();
        for i in 0..self.state.len() {
            let Some(p) = self.segments[i].parent else {
                continue;
            };
            let role = self.segments[i].role;
            // The arms only: the legs follow the steps' pins, which must not be fought.
            if !role.is_arm() {
                continue;
            }
            let most = role.spin() * t.arm_spin_room;
            let knee = most * SPIN_KNEE;
            let was = qmul(qconj(before[p].q), before[i].q);
            let now = qmul(qconj(self.state[p].q), self.state[i].q);
            let d = qlog(qmul(now, qconj(was)));
            let speed = length(d) / h;
            if !(speed > knee && speed.is_finite() && most > knee) {
                continue;
            }
            let allowed = knee + (most - knee) * ((speed - knee) / (most - knee)).tanh();
            let joint = self.joint(i);
            let rel = qmul(qexp(scale(d, allowed / speed)), was);
            let q = qnorm(qmul(self.state[p].q, rel));
            self.state[i].q = q;
            self.state[i].com = add(joint, qrot(q, scale(self.segments[i].seg, 0.5)));
        }
    }

    /// The hits going in over a small step `h`: each its share of [`HIT_TIME`] (eased).
    fn push_in(&mut self, h: f32) {
        let time = tuning::get().hit_time.max(1e-3);
        let mut pushes = std::mem::take(&mut self.pushes);
        for (i, j, local, t) in &mut pushes {
            // Before its delay is over (`t` below 0) nothing goes in yet.
            let share = ease((*t + h) / time) - ease(*t / time);
            *t += h;
            let s = self.state[*i];
            let at = add(s.com, qrot(s.q, *local));
            self.impulse(*i, scale(*j, share), at);
        }
        pushes.retain(|p| p.3 < time);
        self.pushes = pushes;
    }

    /// Each joint's motor stiffness and damping (N m / rad, N m s / rad) at `strength`, and the
    /// pins' and the support's (N / m, N s / m; the support's turn too).
    fn gains(&self, targets: &Targets, strength: f32) -> Gains {
        let t = tuning::get();
        let count = self.state.len();
        let mut joints = vec![(0.0, 0.0); count];
        for (i, joint) in joints.iter_mut().enumerate() {
            if self.segments[i].parent.is_none() {
                continue;
            }
            let (hz, _) = self.segments[i].role.spring();
            let w0 = std::f32::consts::TAU * hz * t.drive_scale;
            // The inertia it moves about its joint: its own and everything below it (a spine
            // carries the chest, the arms and the head: tuned for less, the chain rang).
            let at = self.joint(i);
            let inertia: f32 = (0..count)
                .filter(|&k| below(&self.segments, k, i))
                .map(|k| {
                    let g = &self.segments[k];
                    let d = length(sub(self.state[k].com, at));
                    let len = length(g.seg);
                    g.mass * (d * d + len * len / 12.0 + g.radius * g.radius / 4.0)
                })
                .sum();
            let s = (strength * (1.0 - self.dip[i])).max(0.02);
            *joint = (
                inertia * w0 * w0 * s,
                2.0 * t.drive_zeta * inertia * w0 * s.sqrt(),
            );
        }
        let support = targets.support.clamp(0.0, 1.0);
        let ws = std::f32::consts::TAU * t.support_hz;
        let wf = std::f32::consts::TAU * t.foot_hz;
        let foot_mass = 12.0 * self.mass / 80.0;
        Gains {
            joints,
            support: (
                self.mass * ws * ws * support,
                2.0 * t.support_zeta * self.mass * ws * support.sqrt(),
            ),
            turn: (
                30.0 * ws * ws * support.max(0.2),
                2.0 * 30.0 * ws * support.max(0.2).sqrt(),
            ),
            foot: (foot_mass * wf * wf, 2.0 * t.foot_zeta * foot_mass * wf),
            hand: (3.0 * wf * wf * 0.25, 2.0 * 3.0 * wf * 0.5),
        }
    }

    /// The motors as soft angular constraints (XPBD: compliance `1 / k`): each bone's rotation
    /// relative to its parent toward its target's; the pelvis's rotation toward its target.
    fn drives(&mut self, targets: &Targets, gains: &Gains, h: f32) {
        for i in 0..self.state.len() {
            let k = gains.joints[i].0;
            let Some(p) = self.segments[i].parent else {
                continue;
            };
            if k <= 0.0 {
                continue;
            }
            let qp = self.state[p].q;
            let rel = qmul(qconj(qp), self.state[i].q);
            let mut want = qmul(qconj(targets.rot[p]), targets.rot[i]);
            let world = targets.world.get(i).copied().unwrap_or(0.0);
            if world > 0.0 {
                // Its target's world rotation, through its parent as it is now.
                want = qslerp(want, qmul(qconj(qp), targets.rot[i]), world);
            }
            let e = qrot(qp, qlog(qmul(want, qconj(rel))));
            self.rotate_pair(Some(p), i, e, 1.0 / (k * h * h));
        }
        let p = self.pelvis;
        let k = gains.turn.0;
        let e = qlog(qmul(targets.rot[p], qconj(self.state[p].q)));
        self.rotate_pair(None, p, e, 1.0 / (k * h * h));
    }

    /// Turns bone `i` by up to `e` (a rotation vector, world) against `p` (or the world), with the
    /// scaled compliance `alpha` (XPBD, one step).
    fn rotate_pair(&mut self, p: Option<usize>, i: usize, e: V3, alpha: f32) {
        let theta = length(e);
        if !(theta.is_finite() && theta > 1e-7) {
            return;
        }
        let nrm = scale(e, 1.0 / theta);
        let ii = self.inv_inertia_world(i);
        let wi = dot(nrm, mv(&ii, nrm));
        let (ip, wp) = match p {
            Some(p) => {
                let ip = self.inv_inertia_world(p);
                let wp = dot(nrm, mv(&ip, nrm));
                (Some(ip), wp)
            }
            None => (None, 0.0),
        };
        let lambda = theta / (wi + wp + alpha);
        let l = scale(nrm, lambda);
        let di = mv(&ii, l);
        let s = &mut self.state[i];
        s.q = qnorm(add4(
            s.q,
            scale4(qmul([0.0, di[0], di[1], di[2]], s.q), 0.5),
        ));
        if let (Some(p), Some(ip)) = (p, ip) {
            let dp = mv(&ip, l);
            let s = &mut self.state[p];
            s.q = qnorm(add4(
                s.q,
                scale4(qmul([0.0, -dp[0], -dp[1], -dp[2]], s.q), 0.5),
            ));
        }
    }

    /// The pelvis's support and the feet's and hands' pins as soft positional constraints toward
    /// their targets (the world).
    fn pins(&mut self, targets: &Targets, gains: &Gains, h: f32) {
        let mut list: Vec<(usize, V3, f32)> = Vec::new();
        if gains.support.0 > 0.0 {
            list.push((self.pelvis, targets.pelvis, gains.support.0));
        }
        list.extend(targets.feet.iter().map(|&(i, at)| (i, at, gains.foot.0)));
        list.extend(targets.hands.iter().map(|&(i, at)| (i, at, gains.hand.0)));
        for (i, at, k) in list {
            if i >= self.state.len() || k <= 0.0 {
                continue;
            }
            let joint = self.joint(i);
            let d = sub(at, joint);
            let c = length(d);
            if c < 1e-7 {
                continue;
            }
            let nrm = scale(d, 1.0 / c);
            let r = sub(joint, self.state[i].com);
            let ii = self.inv_inertia_world(i);
            let rn = cross(r, nrm);
            let w = 1.0 / self.segments[i].mass + dot(rn, mv(&ii, rn));
            let lambda = c / (w + 1.0 / (k * h * h));
            self.move_body(i, scale(nrm, lambda), r, &ii);
        }
    }

    /// The motors', the support's and the pins' damping, implicit on the velocities (never past
    /// stopping the motion they damp); no joint turning against its parent faster than its role
    /// allows ([`SPIN_ROOM`] times its spin).
    fn damp(&mut self, targets: &Targets, gains: &Gains, h: f32) {
        self.limit_spin();
        for i in 0..self.state.len() {
            let c = gains.joints[i].1;
            let Some(p) = self.segments[i].parent else {
                continue;
            };
            let rel = sub(self.state[i].w, self.state[p].w);
            let speed = length(rel);
            if c <= 0.0 || speed < 1e-6 {
                continue;
            }
            let nrm = scale(rel, 1.0 / speed);
            let ii = self.inv_inertia_world(i);
            let ip = self.inv_inertia_world(p);
            let w = dot(nrm, mv(&ii, nrm)) + dot(nrm, mv(&ip, nrm));
            let l = speed * (c * h * w / (1.0 + c * h * w)) / w.max(1e-9);
            let li = mv(&ii, scale(nrm, l));
            let lp = mv(&ip, scale(nrm, l));
            self.state[i].w = sub(self.state[i].w, li);
            self.state[p].w = add(self.state[p].w, lp);
        }
        // The pelvis's turn.
        {
            let p = self.pelvis;
            let c = gains.turn.1;
            let w = self.state[p].w;
            let speed = length(w);
            if speed > 1e-6 {
                let nrm = scale(w, 1.0 / speed);
                let ii = self.inv_inertia_world(p);
                let wi = dot(nrm, mv(&ii, nrm));
                let l = speed * (c * h * wi / (1.0 + c * h * wi)) / wi.max(1e-9);
                self.state[p].w = sub(w, mv(&ii, scale(nrm, l)));
            }
        }
        let mut list: Vec<(usize, f32)> = Vec::new();
        if gains.support.1 > 0.0 {
            list.push((self.pelvis, gains.support.1));
        }
        list.extend(targets.feet.iter().map(|&(i, _)| (i, gains.foot.1)));
        list.extend(targets.hands.iter().map(|&(i, _)| (i, gains.hand.1)));
        for (i, c) in list {
            if i >= self.state.len() {
                continue;
            }
            let joint = self.joint(i);
            let r = sub(joint, self.state[i].com);
            let v = add(self.state[i].v, cross(self.state[i].w, r));
            let speed = length(v);
            if speed < 1e-6 {
                continue;
            }
            let nrm = scale(v, 1.0 / speed);
            let ii = self.inv_inertia_world(i);
            let rn = cross(r, nrm);
            let w = 1.0 / self.segments[i].mass + dot(rn, mv(&ii, rn));
            let l = speed * (c * h * w / (1.0 + c * h * w)) / w.max(1e-9);
            let j = scale(nrm, -l);
            let s = &mut self.state[i];
            s.v = add(s.v, scale(j, 1.0 / self.segments[i].mass));
            s.w = add(s.w, mv(&ii, cross(r, j)));
        }
    }

    /// Each joint held together: the child's joint onto its anchor in the parent (positional, no
    /// compliance), the correction shared by the two bodies' generalized inverse masses.
    fn hold_joints(&mut self) {
        for i in 0..self.state.len() {
            let Some(p) = self.segments[i].parent else {
                continue;
            };
            let a = add(
                self.joint(p),
                qrot(self.state[p].q, self.segments[i].anchor),
            );
            let b = self.joint(i);
            let d = sub(a, b);
            let c = length(d);
            if c < 1e-7 {
                continue;
            }
            let nrm = scale(d, 1.0 / c);
            let rp = sub(a, self.state[p].com);
            let ri = sub(b, self.state[i].com);
            let ip = self.inv_inertia_world(p);
            let ii = self.inv_inertia_world(i);
            let rpn = cross(rp, nrm);
            let rin = cross(ri, nrm);
            let wp = 1.0 / self.segments[p].mass + dot(rpn, mv(&ip, rpn));
            let wi = 1.0 / self.segments[i].mass + dot(rin, mv(&ii, rin));
            let lambda = c / (wp + wi);
            let j = scale(nrm, lambda);
            // The child moves toward the anchor, the parent away from the child.
            self.move_body(i, j, ri, &ii);
            self.move_body(p, scale(j, -1.0), rp, &ip);
        }
    }

    fn move_body(&mut self, i: usize, j: V3, r: V3, ii: &M3) {
        let m = self.segments[i].mass;
        let s = &mut self.state[i];
        s.com = add(s.com, scale(j, 1.0 / m));
        let dq = mv(ii, cross(r, j));
        s.q = qnorm(add4(
            s.q,
            scale4(qmul([0.0, dq[0], dq[1], dq[2]], s.q), 0.5),
        ));
    }

    /// No joint turns more than [`MAX_BEND`] (or [`BEND_ROOM`] times its role's limit) from its target (relative to its parent): past it,
    /// the child is turned back (the limbs never fold through themselves on a big hit).
    fn limit_bends(&mut self, targets: &Targets) {
        let t = tuning::get();
        for i in 0..self.state.len() {
            let Some(p) = self.segments[i].parent else {
                continue;
            };
            // A person's joints: each role's own room (a collarbone or the spine little, the
            // shoulder and the elbow more), never past the body's bound.
            let most = (self.segments[i].role.limit() * t.bend_room).min(t.max_bend);
            let qp = self.state[p].q;
            let rel = qmul(qconj(qp), self.state[i].q);
            let want = qmul(qconj(targets.rot[p]), targets.rot[i]);
            let e = qlog(qmul(rel, qconj(want)));
            let a = length(e);
            if a > most {
                let back = scale(e, most / a);
                // The child's rotation set to the target's turned by the limit, about its joint.
                let joint = self.joint(i);
                let q = qnorm(qmul(qp, qmul(qexp(back), want)));
                self.state[i].q = q;
                let seg = self.segments[i].seg;
                self.state[i].com = add(joint, qrot(q, scale(seg, 0.5)));
            }
        }
    }

    /// The bones' world rotations and joints now (m).
    pub fn pose(&self) -> Vec<(Q, V3)> {
        (0..self.state.len())
            .map(|i| (self.state[i].q, self.joint(i)))
            .collect()
    }

    /// The body checked after a step: every bone's motion finite, held to [`MAX_SPEED`] and
    /// [`MAX_SPIN`], every bone within [`MAX_SPREAD`] of the pelvis. `Err` says what went wrong
    /// (the caller drops the body back to the animation).
    pub fn check(&mut self) -> Result<(), String> {
        let pelvis = self
            .state
            .get(self.pelvis)
            .map(|s| s.com)
            .unwrap_or([0.0; 3]);
        for (i, s) in self.state.iter_mut().enumerate() {
            let finite = s
                .com
                .iter()
                .chain(s.v.iter())
                .chain(s.w.iter())
                .chain(s.q.iter())
                .all(|x| x.is_finite());
            if !finite {
                return Err(format!("bone {i}'s motion is not finite"));
            }
            if length(sub(s.com, pelvis)) > MAX_SPREAD {
                return Err(format!(
                    "bone {i} is {:.2} m from the pelvis",
                    length(sub(s.com, pelvis))
                ));
            }
            let v = length(s.v);
            if v > MAX_SPEED {
                s.v = scale(s.v, MAX_SPEED / v);
            }
            let w = length(s.w);
            if w > MAX_SPIN {
                s.w = scale(s.w, MAX_SPIN / w);
            }
        }
        Ok(())
    }

    /// The fastest the body's centre of mass moves (m/s).
    pub fn com_speed(&self) -> f32 {
        let mut p = [0.0; 3];
        for (s, g) in self.state.iter().zip(&self.segments) {
            p = add(p, scale(s.v, g.mass));
        }
        length(p) / self.mass.max(1e-3)
    }
}

/// Whether bone `k` is `i` or hangs below it.
fn below(segments: &[Segment], k: usize, i: usize) -> bool {
    let mut at = Some(k);
    let mut up = 0;
    while let Some(b) = at {
        if b == i {
            return true;
        }
        at = segments[b].parent;
        up += 1;
        if up > 64 {
            return false;
        }
    }
    false
}

/// The gains of a frame ([`Body::gains`]): per joint, the support (position and turn), a foot's
/// pin, a hand's.
struct Gains {
    joints: Vec<(f32, f32)>,
    support: (f32, f32),
    turn: (f32, f32),
    foot: (f32, f32),
    hand: (f32, f32),
}

fn add4(a: Q, b: Q) -> Q {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}
fn scale4(a: Q, k: f32) -> Q {
    [a[0] * k, a[1] * k, a[2] * k, a[3] * k]
}

/// A hinge's bend: the child's direction `f` against the parent's `u` (both in the parent's
/// frame, unit), about the hinge's `axis` (a positive turn flexes it): the flex (rad, negative
/// bent backwards) and the lean out of the hinge's plane (rad).
pub fn hinge_angles(u: V3, f: V3, axis: V3) -> (f32, f32) {
    let n = unit(axis).unwrap_or([1.0, 0.0, 0.0]);
    let lean = dot(f, n).clamp(-1.0, 1.0).asin();
    let inplane = sub(f, scale(n, dot(f, n)));
    let flex = dot(cross(u, inplane), n).atan2(dot(u, inplane));
    (flex, lean)
}

/// The turn (in the parent's frame) that brings a hinge's child direction `f` back within its
/// human range, flex from `-HYPEREXTEND` to `flex`, lean within `lean` (see [`hinge_angles`]),
/// or `None` when it is within it. A child folded far backwards goes to whichever end of the
/// range is nearer round the circle.
pub fn hinge_limit(u: V3, f: V3, axis: V3, flex: f32, lean: f32) -> Option<Q> {
    let n = unit(axis)?;
    let (a, l) = hinge_angles(u, f, n);
    let tau = std::f32::consts::TAU;
    let a2 = if a < -HYPEREXTEND {
        let to_low = -HYPEREXTEND - a;
        let to_high = (a + tau) - flex;
        if to_high < to_low {
            flex
        } else {
            -HYPEREXTEND
        }
    } else {
        a.min(flex)
    };
    let l2 = l.clamp(-lean, lean);
    if (a2 - a).abs() < 1e-5 && (l2 - l).abs() < 1e-5 {
        return None;
    }
    // The wanted direction: `u` turned about `n` by the flex, leaned toward `n`.
    let inplane = qrot(qexp(scale(n, a2)), u);
    let want = add(scale(inplane, l2.cos()), scale(n, l2.sin()));
    let axis = cross(f, want);
    let s = length(axis);
    let c = dot(f, want).clamp(-1.0, 1.0);
    if s < 1e-6 {
        if c > 0.0 {
            return None;
        }
        return Some(qexp(scale(n, std::f32::consts::PI)));
    }
    Some(qexp(scale(axis, s.atan2(c) / s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A standing figure: pelvis, a spine up, an arm out, a leg down.
    fn figure() -> Vec<Bone> {
        let b = |parent, role, joint: V3, end: V3, mass| Bone {
            parent,
            role,
            q: Q_ID,
            joint,
            end,
            radius: 0.06,
            mass,
            before: None,
        };
        vec![
            b(None, Role::Pelvis, [0.0, 0.0, 0.95], [0.0, 0.0, 1.05], 11.0),
            b(
                Some(0),
                Role::Spine,
                [0.0, 0.0, 1.05],
                [0.0, 0.0, 1.45],
                30.0,
            ),
            b(Some(1), Role::Head, [0.0, 0.0, 1.45], [0.0, 0.0, 1.7], 6.0),
            b(
                Some(1),
                Role::UpperArm,
                [0.2, 0.0, 1.4],
                [0.2, 0.0, 1.1],
                2.5,
            ),
            b(
                Some(3),
                Role::Forearm,
                [0.2, 0.0, 1.1],
                [0.2, 0.0, 0.85],
                1.5,
            ),
            b(Some(0), Role::Thigh, [0.1, 0.0, 0.95], [0.1, 0.0, 0.5], 8.0),
            b(Some(5), Role::Calf, [0.1, 0.0, 0.5], [0.1, 0.0, 0.08], 4.0),
            b(
                Some(6),
                Role::Foot,
                [0.1, 0.0, 0.08],
                [0.1, 0.12, 0.02],
                1.2,
            ),
        ]
    }

    fn targets(body: &Body) -> Targets {
        Targets {
            rot: vec![Q_ID; body.state.len()],
            pelvis: [0.0, 0.0, 0.95],
            feet: vec![(7, [0.1, 0.0, 0.08])],
            hands: vec![],
            support: 1.0,
            world: vec![],
            anim: vec![],
            forward: [0.0; 3],
        }
    }

    #[test]
    fn quaternions() {
        let w = [0.3, -0.5, 0.9];
        assert!(length(sub(qlog(qexp(w)), w)) < 1e-5);
        let m = crate::flinch::rotation(w);
        let q = qfrom_m(&m);
        let back = m_from_q(q);
        for (a, b) in m.iter().zip(back.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
        let v = [1.0, 2.0, 3.0];
        let a = crate::flinch::apply_m(&m, v);
        let b = qrot(q, v);
        assert!(length(sub(a, b)) < 1e-5);
        assert!((ease(0.5) - 0.5).abs() < 1e-6 && ease(0.0) == 0.0 && ease(1.0) == 1.0);
    }

    /// Left alone with its targets the body holds its pose and stays together.
    #[test]
    fn it_stands() {
        let mut body = Body::from_pose(&figure(), 1.0 / 60.0);
        let t = targets(&body);
        for _ in 0..120 {
            body.step(&t, 1.0 / 60.0, 1.0);
        }
        assert!(
            length(sub(body.joint(0), [0.0, 0.0, 0.95])) < 0.05,
            "{:?}",
            body.joint(0)
        );
        for i in 1..body.state.len() {
            let p = body.segments[i].parent.unwrap();
            let a = add(
                body.joint(p),
                qrot(body.state[p].q, body.segments[i].anchor),
            );
            assert!(length(sub(a, body.joint(i))) < 1e-3, "joint {i} apart");
        }
        assert!(body.com_speed() < 0.05);
    }

    /// A hit on the arm moves the arm first and the head after (the push travels up the chain);
    /// it all settles back with no jump in any velocity.
    #[test]
    fn a_hit_travels_and_settles() {
        let mut body = Body::from_pose(&figure(), 1.0 / 60.0);
        let t = targets(&body);
        body.hit(20.0, [0.0, 1.0, 0.0], [0.2, 0.0, 0.95], 0.4, &[0.0; 8], &[]);
        let mut arm: Vec<f32> = Vec::new();
        let mut spine: Vec<f32> = Vec::new();
        let mut biggest_jump = 0.0f32;
        let mut last: Vec<V3> = body.state.iter().map(|s| s.v).collect();
        for frame in 0..180 {
            body.step(&t, 1.0 / 60.0, strength(frame as f32 / 60.0));
            arm.push(length(body.state[4].v));
            spine.push(length(body.state[2].w));
            for (s, l) in body.state.iter().zip(&last) {
                biggest_jump = biggest_jump.max(length(sub(s.v, *l)));
            }
            last = body.state.iter().map(|s| s.v).collect();
        }
        // When each starts moving: a quarter of its peak.
        let onset = |v: &[f32]| {
            let peak = v.iter().cloned().fold(0.0f32, f32::max);
            (peak, v.iter().position(|x| *x > peak / 4.0).unwrap_or(0))
        };
        let (arm_peak, arm_at) = onset(&arm);
        let (spine_peak, spine_at) = onset(&spine);
        assert!(arm_peak > 0.5, "the struck arm moves: {arm_peak}");
        assert!(spine_peak > 0.1, "the spine turns: {spine_peak}");
        assert!(
            arm_at < spine_at,
            "the arm first ({arm_at}), the spine after ({spine_at})"
        );
        assert!(biggest_jump < 0.6, "no velocity jumps: {biggest_jump}");
        assert!(body.com_speed() < 0.1, "settled: {}", body.com_speed());
        assert!(length(sub(body.joint(0), [0.0, 0.0, 0.95])) < 0.05);
    }

    #[test]
    fn the_weights_ease() {
        assert_eq!(weight(0.0), 1.0);
        assert_eq!(weight(ACTIVE_FOR), 1.0);
        assert!((weight(ACTIVE_FOR + BLEND_BACK / 2.0) - 0.5).abs() < 1e-4);
        assert_eq!(weight(ACTIVE_FOR + BLEND_BACK), 0.0);
        assert!((strength(0.0) - DRIVE_SOFT).abs() < 1e-6);
        assert_eq!(strength(DRIVE_RAMP), 1.0);
    }

    /// A hinge past straight or past its most flex is turned back to the nearer stop; within its
    /// range it is left alone.
    #[test]
    fn hinges_stop_both_ways() {
        let u = [0.0, 0.0, -1.0];
        let axis = [1.0, 0.0, 0.0];
        let bend = |a: f32| qrot(qexp(scale(axis, a)), u);
        assert!(hinge_limit(u, bend(1.0), axis, ELBOW_FLEX, ELBOW_LEAN).is_none());
        for (a, want) in [(-0.4, -HYPEREXTEND), (2.9, ELBOW_FLEX), (-2.7, ELBOW_FLEX)] {
            let f = bend(a);
            let t = hinge_limit(u, f, axis, ELBOW_FLEX, ELBOW_LEAN).expect("past a stop");
            let (flex, _) = hinge_angles(u, qrot(t, f), axis);
            assert!((flex - want).abs() < 1e-3, "{a}: {flex} vs {want}");
        }
        // A lean out of the plane is held to its bound.
        let leaned = add(scale(bend(0.5), 0.6f32.cos()), scale(axis, 0.6f32.sin()));
        let t = hinge_limit(u, leaned, axis, ELBOW_FLEX, ELBOW_LEAN).expect("leaned too far");
        let (_, lean) = hinge_angles(u, qrot(t, leaned), axis);
        assert!((lean - ELBOW_LEAN).abs() < 1e-3);
    }
}
