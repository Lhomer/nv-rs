//! The active ragdoll's pure parts (stage 1 flinch, stage 3 timings): the bone roles and their
//! springs, the kick's spread, the vector and matrix helpers, the brace's timing. The host's
//! `crate::active_ragdoll` re-exports this module and adds the game's side (its bones, the pose
//! layer, the hits); `tools/ragdoll-lab` adds a capsule mannequin's.
//!
//! The constants are the game's tuning; the functions read the values through
//! [`crate::tuning::get`], which in the game is always these constants.

use crate::tuning;

/// A bone's role: its spring, its share of a kick (light parts move more), how far it may turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Pelvis,
    Spine,
    Neck,
    Head,
    Clavicle,
    UpperArm,
    Forearm,
    Thigh,
    Calf,
    Foot,
    Hand,
}

/// Every role, in the order of the tables below.
pub const ROLES: [Role; 11] = [
    Role::Pelvis,
    Role::Spine,
    Role::Neck,
    Role::Head,
    Role::Clavicle,
    Role::UpperArm,
    Role::Forearm,
    Role::Thigh,
    Role::Calf,
    Role::Foot,
    Role::Hand,
];

/// Each role's spring: frequency (Hz) and damping ratio (under 1: an overshoot).
pub const SPRINGS: [(f32, f32); 11] = [
    (3.2, 0.5),
    // The spine gives under a hit (the active body's torso sways, then comes back).
    (2.2, 0.45),
    // The neck and the head: firm enough that a light hit nods them, not whips them (the active
    // body, `crate::active`).
    (3.4, 0.5),
    (3.0, 0.45),
    // The arms (Patrick, 2026-11-15: they snapped and flailed): softer and well damped, so they
    // swing out and settle like a person's instead of whipping.
    (2.0, 0.6),
    (1.3, 0.65),
    (1.5, 0.7),
    (3.0, 0.5),
    (3.2, 0.5),
    (3.4, 0.6),
    (1.7, 0.75),
];
/// How much of a kick each role takes (its lightness).
pub const GAINS: [f32; 11] = [0.35, 0.7, 0.8, 0.9, 0.6, 0.75, 0.65, 0.6, 0.7, 0.3, 0.55];
/// The most each role turns (radians).
pub const LIMITS: [f32; 11] = [0.2, 0.35, 0.4, 0.5, 0.45, 0.9, 0.7, 0.45, 0.6, 0.4, 0.6];
/// The fastest each role turns (rad/s): a person's joints, the arms the slowest (no whipping).
pub const SPINS: [f32; 11] = [6.0, 7.0, 8.0, 9.0, 4.0, 5.0, 6.0, 6.0, 7.0, 6.0, 6.0];

impl Role {
    /// The role's index in [`ROLES`] and the tables.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The role's name (as the lab's sliders and the ini export write it).
    pub fn name(self) -> &'static str {
        match self {
            Role::Pelvis => "Pelvis",
            Role::Spine => "Spine",
            Role::Neck => "Neck",
            Role::Head => "Head",
            Role::Clavicle => "Clavicle",
            Role::UpperArm => "UpperArm",
            Role::Forearm => "Forearm",
            Role::Thigh => "Thigh",
            Role::Calf => "Calf",
            Role::Foot => "Foot",
            Role::Hand => "Hand",
        }
    }

    /// The spring's frequency (Hz) and damping ratio (under 1: an overshoot).
    pub fn spring(self) -> (f32, f32) {
        let r = tuning::get().roles[self.index()];
        (r.hz, r.zeta)
    }

    /// How much of a kick the bone takes (its lightness).
    pub fn gain(self) -> f32 {
        tuning::get().roles[self.index()].gain
    }

    /// The most the bone turns (radians).
    pub fn limit(self) -> f32 {
        tuning::get().roles[self.index()].limit
    }

    /// The fastest the bone turns (rad/s).
    pub fn spin(self) -> f32 {
        tuning::get().roles[self.index()].spin
    }

    /// An arm's bone (the clavicle to the hand): it follows its poses with a lag
    /// ([`Follow`]) and is the part the aim keeps ([`hold_aim`]).
    pub fn is_arm(self) -> bool {
        matches!(
            self,
            Role::Clavicle | Role::UpperArm | Role::Forearm | Role::Hand
        )
    }
}

/// The rig's bones by name (the Gamebryo biped's), and their roles.
pub const BONES: [(&str, Role); 20] = [
    ("Bip01 Pelvis", Role::Pelvis),
    ("Bip01 Spine", Role::Spine),
    ("Bip01 Spine1", Role::Spine),
    ("Bip01 Spine2", Role::Spine),
    ("Bip01 Neck1", Role::Neck),
    ("Bip01 Head", Role::Head),
    ("Bip01 L Clavicle", Role::Clavicle),
    ("Bip01 R Clavicle", Role::Clavicle),
    ("Bip01 L UpperArm", Role::UpperArm),
    ("Bip01 R UpperArm", Role::UpperArm),
    ("Bip01 L Forearm", Role::Forearm),
    ("Bip01 R Forearm", Role::Forearm),
    ("Bip01 L Thigh", Role::Thigh),
    ("Bip01 R Thigh", Role::Thigh),
    ("Bip01 L Calf", Role::Calf),
    ("Bip01 R Calf", Role::Calf),
    ("Bip01 L Foot", Role::Foot),
    ("Bip01 R Foot", Role::Foot),
    ("Bip01 L Hand", Role::Hand),
    ("Bip01 R Hand", Role::Hand),
];

/// The tuning (the host's `crate::active_ragdoll` documents it): kick (rad/s per m/s of the body's
/// speed), the spread's reach (m) at no speed and per m/s, at most; the muscle's dip near the hit
/// and how fast it comes back (s); the head's lag (a share of the spine's kick against it); the
/// knee's buckle and the pelvis's drop (m/s per m/s) for a leg hit; when a spring is at rest; the
/// frame time at most.
pub const KICK: f32 = 16.0;
pub const SPREAD_BASE: f32 = 0.25;
pub const SPREAD_PER_MPS: f32 = 0.4;
pub const SPREAD_MAX: f32 = 1.5;
pub const MUSCLE_DIP: f32 = 0.6;
pub const MUSCLE_BACK: f32 = 0.25;
pub const HEAD_LAG: f32 = 0.6;
pub const KNEE_BUCKLE: f32 = 6.0;
pub const PELVIS_DROP: f32 = 0.6;
pub const PELVIS_DROP_MAX: f32 = 0.12;
pub const REST: f32 = 0.002;
pub const MAX_DT: f32 = 0.05;
pub const UNITS_PER_METRE: f32 = crate::UNITS_PER_METRE;

/// How far (m) a kick spreads for a body speed `v` (m/s).
pub fn spread_of(v: f32) -> f32 {
    let t = tuning::get();
    (t.spread_base + t.spread_per_mps * v.max(0.0)).min(t.spread_max)
}

/// A bone's weight in a kick: its role's gain, fading with its distance (m) from the impact.
pub fn weight(role: Role, distance: f32, spread: f32) -> f32 {
    role.gain() * (-distance.max(0.0) / spread.max(0.05)).exp()
}

// --- Vectors and matrices (row-major 3 x 3, as `NiMatrix3`). -----------------------------------

pub type V3 = [f32; 3];
pub type M3 = [f32; 9];

/// The identity rotation.
pub const IDENTITY: M3 = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}
pub fn unit(a: V3) -> Option<V3> {
    let l = length(a);
    (l.is_finite() && l > 1e-6).then(|| scale(a, 1.0 / l))
}

pub fn mul(a: &M3, b: &M3) -> M3 {
    let mut m = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            m[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    m
}
pub fn transpose(a: &M3) -> M3 {
    [a[0], a[3], a[6], a[1], a[4], a[7], a[2], a[5], a[8]]
}
pub fn apply_m(a: &M3, v: V3) -> V3 {
    [
        a[0] * v[0] + a[1] * v[1] + a[2] * v[2],
        a[3] * v[0] + a[4] * v[1] + a[5] * v[2],
        a[6] * v[0] + a[7] * v[1] + a[8] * v[2],
    ]
}

/// The rotation by the rotation vector `w` (its direction the axis, its length the angle).
pub fn rotation(w: V3) -> M3 {
    let angle = length(w);
    if !angle.is_finite() || angle < 1e-7 {
        return IDENTITY;
    }
    let [x, y, z] = scale(w, 1.0 / angle);
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    [
        t * x * x + c,
        t * x * y - s * z,
        t * x * z + s * y,
        t * x * y + s * z,
        t * y * y + c,
        t * y * z - s * x,
        t * x * z - s * y,
        t * y * z + s * x,
        t * z * z + c,
    ]
}

// --- The springs. --------------------------------------------------------------------------------

/// A bone's offset from its animated pose: a rotation vector (world space) and its angular
/// velocity, its muscle's dip and when it was hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub angle: V3,
    pub velocity: V3,
    pub dip: f32,
}

impl Spring {
    pub const REST: Spring = Spring {
        angle: [0.0; 3],
        velocity: [0.0; 3],
        dip: 0.0,
    };

    /// One step of `dt` seconds of a bone of `role`: pulled back to its pose (its muscle lowered by
    /// the dip, which fades), damped, held within the role's limit.
    pub fn step(&mut self, role: Role, dt: f32) {
        let (f, zeta) = role.spring();
        let w0 = std::f32::consts::TAU * f;
        let muscle = 1.0 - self.dip.clamp(0.0, 0.9);
        let k = w0 * w0 * muscle;
        let c = 2.0 * zeta * w0 * muscle.sqrt();
        let dt = dt.clamp(0.0, MAX_DT);
        // No joint turns faster than a person's (the arms the slowest): a kick past it is held to
        // it before it moves the bone.
        let most = role.spin();
        let cap = |v: V3| {
            let w = length(v);
            if w > most && w.is_finite() {
                scale(v, most / w)
            } else {
                v
            }
        };
        self.velocity = cap(self.velocity);
        // Semi-implicit Euler, in two halves for the stiff springs.
        for _ in 0..2 {
            let h = dt / 2.0;
            let acc = sub(scale(self.angle, -k), scale(self.velocity, c));
            self.velocity = cap(add(self.velocity, scale(acc, h)));
            self.angle = add(self.angle, scale(self.velocity, h));
        }
        let a = length(self.angle);
        if a > role.limit() {
            self.angle = scale(self.angle, role.limit() / a);
            // At the limit the joint stops it: the velocity outward is taken off.
            if let Some(u) = unit(self.angle) {
                let out = dot(self.velocity, u);
                if out > 0.0 {
                    self.velocity = sub(self.velocity, scale(u, out));
                }
            }
        }
        self.dip *= (-dt / tuning::get().muscle_back).exp();
    }

    pub fn at_rest(&self) -> bool {
        length(self.angle) < REST && length(self.velocity) < REST * 10.0
    }
}

// --- Stage 3. ------------------------------------------------------------------------------------

/// Stage 3's timings: the brace's reach, hold and release (s); the fall's lead for a knockdown, a
/// ledge (windmilling) and a fall at the end of the steps (s); the windmill's turns a second and
/// swing (rad); how much the torso and the knees go over in a fall (rad), how far the pelvis goes
/// (units); the push a wall brace holds (m/s), past it the body crumples against the wall.
pub const BRACE_REACH: f32 = 0.7;
pub const BRACE_HOLD: f32 = 0.5;
pub const BRACE_RELEASE: f32 = 0.6;
pub const FALL_LEAD: f32 = 0.12;
pub const LEDGE_LEAD: f32 = 0.4;
pub const OVERRUN_LEAD: f32 = 0.1;
pub const WINDMILL_HZ: f32 = 1.1;
pub const WINDMILL_SWING: f32 = 1.1;
pub const FALL_LEAN: f32 = 0.4;
pub const FALL_KNEES: f32 = 0.3;
pub const FALL_SHIFT: f32 = 25.0;
pub const BRACE_HOLDS: f32 = 2.5;
/// Where the hands meet the wall: this far ahead of the shoulders along the push and this far
/// below them (m): chest high, a deliberate reach (Patrick, 2026-11-15: it was rubbery and high).
pub const BRACE_OUT: f32 = 0.45;
pub const BRACE_LOW: f32 = 0.28;
/// The bracing arms' motors this many times firmer (the active body's dip below 0): a deliberate
/// reach and a steady hold, not a rubbery one.
pub const BRACE_FIRM: f32 = 1.0;
/// How much the bracing arms track their world rotation (1: the wall point, whatever the torso).
pub const BRACE_WORLD: f32 = 0.5;

/// Where a brace's hand goes from its shoulder `shoulder` along the push `dir` (game units): its
/// reach ahead, or (a wall found `wall` units ahead by a ray cast) the wall's face less a hand,
/// or (a waist-high obstacle, its top at `top`, world z) on its top just past its edge.
pub fn brace_target(shoulder: V3, dir: V3, wall: Option<f32>, top: Option<f32>) -> V3 {
    let t = tuning::get();
    let mut out = t.brace_reach_out * UNITS_PER_METRE;
    // A waist-high obstacle (a railing, a table, a half-wall; `crate::obstacle`): the hands on its
    // top, just past its edge.
    if let (Some(w), Some(z)) = (
        wall.filter(|w| w.is_finite() && *w > 0.0),
        top.filter(|z| z.is_finite()),
    ) {
        return add(
            add(shoulder, scale(dir, w + BRACE_ON_TOP)),
            [0.0, 0.0, z + BRACE_HAND - shoulder[2]],
        );
    }
    if let Some(w) = wall.filter(|w| w.is_finite() && *w > 0.0) {
        out = out.min((w - BRACE_HAND).max(0.2 * UNITS_PER_METRE));
    }
    add(
        shoulder,
        add(
            scale(dir, out),
            [0.0, 0.0, -t.brace_reach_low * UNITS_PER_METRE],
        ),
    )
}

/// A hand's thickness short of the wall (game units).
pub const BRACE_HAND: f32 = 5.0;
/// How far past a waist-high obstacle's edge the hands go onto its top (game units).
pub const BRACE_ON_TOP: f32 = 4.0;

/// How far behind the body an arm may reach from the shoulder (rad back from straight down: a
/// person's shoulder extension, about 45 degrees), and how far out to the side a reach behind goes
/// (a fraction of its length).
pub const SHOULDER_BACK: f32 = 0.8;
pub const BACK_OUT: f32 = 0.25;

/// A hand's goal `goal` held to where a person's arm can reach from its shoulder `shoulder`, for
/// an actor facing `forward` (flat) and an arm on `side` (0 left, 1 right): a reach behind the body
/// is turned down (its length kept) until it is no further back than [`SHOULDER_BACK`] from
/// straight down, and goes a little out to the side; a hand reaching back to a wall goes back,
/// down and out, palms low, as a person's does, never up behind the shoulder.
pub fn human_reach(shoulder: V3, goal: V3, forward: V3, side: u8) -> V3 {
    let Some(fwd) = unit([forward[0], forward[1], 0.0]) else {
        return goal;
    };
    let out = side_out(fwd, side);
    let d = sub(goal, shoulder);
    let back = -dot(d, fwd);
    if back <= 0.0 {
        return goal;
    }
    // In the plane of the back and down: its angle back from straight down.
    let lateral = dot(d, out);
    let sideways = scale(out, lateral);
    let rest = sub(d, sideways);
    let len = length(rest);
    let down = -rest[2];
    let angle = back.atan2(down);
    if angle <= SHOULDER_BACK || len < 1e-3 {
        return goal;
    }
    let (sin, cos) = SHOULDER_BACK.sin_cos();
    let turned = add(scale(fwd, -len * sin), [0.0, 0.0, -len * cos]);
    let wide = lateral.max(len * BACK_OUT);
    add(shoulder, add(turned, scale(out, wide)))
}

/// The way out to the side for an arm on `side` (0 left, 1 right) of an actor facing `fwd`.
fn side_out(fwd: V3, side: u8) -> V3 {
    let right = cross(fwd, [0.0, 0.0, 1.0]);
    if side == 0 {
        scale(right, -1.0)
    } else {
        right
    }
}

/// The rotation taking the frame of `a` and `b` (directions, `b` made square to `a`) onto that of
/// `c` and `d`; `None` when either pair is degenerate.
fn align(a: V3, b: V3, c: V3, d: V3) -> Option<M3> {
    let frame = |x: V3, y: V3| -> Option<M3> {
        let x = unit(x)?;
        let y = unit(sub(y, scale(x, dot(x, y))))?;
        let z = cross(x, y);
        // Columns x, y, z (row-major).
        Some([x[0], y[0], z[0], x[1], y[1], z[1], x[2], y[2], z[2]])
    };
    Some(mul(&frame(c, d)?, &transpose(&frame(a, b)?)))
}

/// Two-bone IK through a one-way hinge (an elbow, a knee): from the animated root, middle and end
/// joints (`root0`, `mid0`, `end0`) to the end at `goal` with the root at `root`; `hinge0` the
/// animation's hinge axis (a turn about it flexes the joint), `out` a nudge for the middle (an
/// elbow a little out to the side). The middle points away from where the flex takes the end (an
/// elbow away from the upper arm's front, a knee forward), and the first bone is turned so that
/// its hinge is the bend's: the joint flexes, never bends backwards. The world turn of the first
/// bone and the second's turn after it (carried down the chain: `r1ᵀ r2 r1`).
pub fn hinge_ik(
    root0: V3,
    mid0: V3,
    end0: V3,
    root: V3,
    goal: V3,
    hinge0: Option<V3>,
    out: V3,
) -> Option<(M3, M3)> {
    use crate::balance::{between, two_bone};
    let (u0, f0) = (sub(mid0, root0), sub(end0, mid0));
    let (l1, l2) = (length(u0), length(f0));
    if l1 < 1.0 || l2 < 1.0 {
        return None;
    }
    let g = unit(sub(goal, root))?;
    let pole = match hinge0 {
        Some(h) => {
            let front = unit(cross(h, g)).unwrap_or([0.0, 0.0, 1.0]);
            let mid = add(root, scale(g, l1 * 0.5));
            add(mid, scale(add(scale(front, -1.0), scale(out, 0.4)), 20.0))
        }
        None => add(add(mid0, sub(root, root0)), [0.0, 0.0, -20.0]),
    };
    let (m, reached) = two_bone(root, goal, l1, l2, pole)?;
    let (u1, f1) = (sub(m, root), sub(reached, m));
    let bent1 = cross(u1, f1);
    let r1 = match hinge0 {
        Some(h0) if length(bent1) > 0.1f32.sin() * l1 * l2 => {
            align(u0, h0, u1, bent1).unwrap_or_else(|| rotation(between(u0, u1)))
        }
        _ => rotation(between(u0, u1)),
    };
    let r2 = rotation(between(apply_m(&r1, f0), f1));
    Some((r1, mul(&mul(&transpose(&r1), &r2), &r1)))
}

/// A hinge's axis from the animated joints (a turn about it flexes the joint): the animation's
/// bend when it is bent enough to say it, else from the way the actor faces (`toward`: where a
/// flex takes the end; forward for an elbow, back for a knee), else the body's right.
fn hinge_axis(root0: V3, mid0: V3, end0: V3, toward: Option<V3>) -> Option<V3> {
    let (u0, f0) = (sub(mid0, root0), sub(end0, mid0));
    let bent = cross(u0, f0);
    if length(bent) > 0.2f32.sin() * length(u0) * length(f0) {
        return unit(bent);
    }
    let t = toward?;
    let a = cross(u0, t);
    if length(a) > 0.3 * length(u0) {
        unit(a)
    } else {
        unit(cross(t, [0.0, 0.0, 1.0]))
    }
}

/// Two-bone IK for an arm reaching its hand to `goal` from the animated shoulder, elbow and hand
/// (game units), as a person's arm does: the goal held to the shoulder's reach
/// ([`human_reach`]), then [`hinge_ik`] with the elbow's hinge (the animation's bend, or a
/// straight arm's that brings the forearm forward) and the elbow a little out.
pub fn reach_arm_ik(
    shoulder: V3,
    elbow0: V3,
    hand0: V3,
    goal: V3,
    forward: V3,
    side: u8,
) -> Option<(M3, M3)> {
    let fwd = unit([forward[0], forward[1], 0.0]);
    let mut goal = human_reach(shoulder, goal, forward, side);
    let hinge = hinge_axis(shoulder, elbow0, hand0, fwd);
    let out = fwd.map_or([0.0; 3], |f| side_out(f, side));
    let mut reach = hinge_ik(shoulder, elbow0, hand0, shoulder, goal, hinge, out)?;
    // The elbow's bend can lift the upper arm further back than the hand: while it is past the
    // shoulder's reach back, the goal is turned down about the shoulder by what is over.
    if let Some(f) = fwd {
        for _ in 0..3 {
            let upper = apply_m(&reach.0, sub(elbow0, shoulder));
            let back = -dot(upper, f);
            let over = back.atan2(-upper[2]) - (SHOULDER_BACK + UPPER_ARM_BACK);
            if back <= 0.0 || over <= 0.0 {
                break;
            }
            // Down about the body's right (a turn about it takes a reach behind toward straight
            // down).
            let axis = cross(f, [0.0, 0.0, 1.0]);
            let turned = apply_m(&rotation(scale(axis, over)), sub(goal, shoulder));
            goal = add(shoulder, turned);
            reach = hinge_ik(shoulder, elbow0, hand0, shoulder, goal, hinge, out)?;
        }
    }
    Some(reach)
}

/// How much further back than the hand (rad past [`SHOULDER_BACK`]) the upper arm may go as the
/// elbow bends (a person's shoulder extends to about 55 degrees).
pub const UPPER_ARM_BACK: f32 = 0.15;

/// Two-bone IK for a leg putting its ankle at `goal` with the hip moved to `hip` (game units),
/// from the animated hip, knee and ankle: [`hinge_ik`] with the knee's hinge (the animation's
/// bend, or a straight leg's that takes the calf back), so the knee points forward and never
/// bends backwards, whichever way the body is pushed. The thigh's world turn, the calf's turn
/// after it and the foot's (kept level: its world rotation as the animation had it).
pub fn leg_ik(
    hip0: V3,
    knee0: V3,
    ankle0: V3,
    hip: V3,
    goal: V3,
    forward: V3,
) -> Option<(M3, M3, M3)> {
    let back = unit([-forward[0], -forward[1], 0.0]);
    let hinge = hinge_axis(hip0, knee0, ankle0, back);
    let (r1, m) = hinge_ik(hip0, knee0, ankle0, hip, goal, hinge, [0.0; 3])?;
    let level = mul(&transpose(&m), &transpose(&r1));
    Some((r1, m, level))
}
/// Bracing on the ground (the host's `crate::knockdown`, the ragdoll's first moments): for this
/// long (s) after the ragdoll takes the body, while the pelvis is more than this high over its
/// lowest bone (units) and dropping faster than this (units a second), the hands and forearms are
/// steered this share a frame toward this speed (m/s along the fall, m/s down).
pub const BRACE_GROUND_SECONDS: f32 = 1.2;
pub const BRACE_GROUND_HEIGHT: f32 = 25.0;
pub const BRACE_GROUND_DROP: f32 = 35.0;
pub const BRACE_GROUND_SHARE: f32 = 0.12;
pub const BRACE_GROUND_SPEED: (f32, f32) = (1.5, 2.5);
/// The ragdoll's arm bones spin at most this fast (rad/s) while it flies and lands.
pub const ARM_RAGDOLL_SPIN: f32 = 8.0;

// --- The arms' lag and follow-through (Patrick, 2026-11-15). -------------------------------------

/// How an arm follows the poses it is given (the steps' arms out, the wall brace's reach, the
/// windmill): a spring of [`ARM_FOLLOW_HZ`] and damping [`ARM_FOLLOW_ZETA`] (a little under 1: a
/// little follow-through past the pose), never faster than [`ARM_FOLLOW_SPIN`].
pub const ARM_FOLLOW_HZ: f32 = 2.6;
pub const ARM_FOLLOW_ZETA: f32 = 0.7;
pub const ARM_FOLLOW_SPIN: f32 = 4.5;

/// A bone's shown offset following a target offset (rotation vectors, world space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Follow {
    pub angle: V3,
    pub velocity: V3,
}

impl Follow {
    pub const REST: Follow = Follow {
        angle: [0.0; 3],
        velocity: [0.0; 3],
    };

    /// One step of `dt` toward `target` (module documentation).
    pub fn step(&mut self, target: V3, dt: f32) {
        let t = tuning::get();
        let w0 = std::f32::consts::TAU * t.arm_follow_hz.max(0.1);
        let c = 2.0 * t.arm_follow_zeta * w0;
        let dt = dt.clamp(0.0, MAX_DT);
        for _ in 0..2 {
            let h = dt / 2.0;
            let acc = sub(
                scale(sub(target, self.angle), w0 * w0),
                scale(self.velocity, c),
            );
            self.velocity = add(self.velocity, scale(acc, h));
            let w = length(self.velocity);
            if w > t.arm_follow_spin && w.is_finite() {
                self.velocity = scale(self.velocity, t.arm_follow_spin / w);
            }
            self.angle = add(self.angle, scale(self.velocity, h));
        }
        if !self.angle.iter().all(|x| x.is_finite()) {
            *self = Follow::REST;
        }
    }

    pub fn at_rest(&self) -> bool {
        length(self.angle) < REST && length(self.velocity) < REST * 10.0
    }
}

/// The rotation vector of a rotation (its axis times its angle; the inverse of [`rotation`]).
pub fn rotation_vector(m: &M3) -> V3 {
    let c = ((m[0] + m[4] + m[8] - 1.0) / 2.0).clamp(-1.0, 1.0);
    let angle = c.acos();
    if !angle.is_finite() || angle < 1e-6 {
        return [0.0; 3];
    }
    let axis = [m[7] - m[5], m[2] - m[6], m[3] - m[1]];
    let s = angle.sin();
    if s.abs() < 1e-4 {
        // Near a half turn: the axis from the diagonal.
        let x = ((m[0] + 1.0) / 2.0).max(0.0).sqrt();
        let y = ((m[4] + 1.0) / 2.0).max(0.0).sqrt().copysign(m[1] + m[3]);
        let z = ((m[8] + 1.0) / 2.0).max(0.0).sqrt().copysign(m[2] + m[6]);
        return unit([x, y, z]).map_or([0.0; 3], |u| scale(u, angle));
    }
    scale(axis, angle / (2.0 * s))
}

// --- Keeping the aim (Patrick, 2026-11-15). ------------------------------------------------------

/// How much an armed body keeps its arms on its aim through a flinch below the stagger: its arms'
/// own kick is cut to `1 - AIM_HOLD` of it and the torso's turn under them taken back by
/// `AIM_HOLD` (a person keeps the gun pointed). A stagger or more lets the aim go (0).
pub const AIM_HOLD: f32 = 0.8;

/// The world offsets `offsets` (each bone's `R`, its world rotation `R_root ... R_parent R anim`)
/// with the aim kept by `hold` (0 to 1): each arm root (an arm bone whose parent is no arm bone)
/// takes back `hold` of its parents' turn; every arm bone keeps `1 - hold` of its own.
/// `parent[b]` is the rig bone `b` hangs from, `roles[b]` its role.
pub fn hold_aim(offsets: &mut [M3], parent: &[Option<usize>], roles: &[Role], hold: f32) {
    let hold = hold.clamp(0.0, 1.0);
    if hold <= 0.0 || offsets.len() != parent.len() || offsets.len() != roles.len() {
        return;
    }
    // The chain above each bone, root first.
    let chain = |b: usize, offsets: &[M3]| {
        let mut list = Vec::new();
        let mut at = parent[b];
        while let Some(p) = at {
            list.push(p);
            at = parent[p];
            if list.len() > 64 {
                break;
            }
        }
        let mut m = IDENTITY;
        for &p in list.iter().rev() {
            m = mul(&m, &offsets[p]);
        }
        m
    };
    let original = offsets.to_vec();
    for b in 0..offsets.len() {
        if !roles[b].is_arm() {
            continue;
        }
        let own = rotation(scale(rotation_vector(&original[b]), 1.0 - hold));
        let root = parent[b].map_or(true, |p| !roles[p].is_arm());
        offsets[b] = if root {
            let back = rotation(scale(
                rotation_vector(&transpose(&chain(b, &original))),
                hold,
            ));
            mul(&back, &own)
        } else {
            own
        };
    }
}

/// The brace's weight `t` seconds in: up over the reach, held, down over the release; `None` when
/// it is over.
pub fn brace_weight(t: f32) -> Option<f32> {
    let k = tuning::get();
    if t < 0.0 {
        return Some(0.0);
    }
    // Eased in and out (`6t^5 - 15t^4 + 10t^3`): the hands set off and arrive at rest, no
    // overshoot asked of the motors.
    if t < k.brace_reach {
        return Some(crate::active::ease(t / k.brace_reach));
    }
    if t < k.brace_reach + k.brace_hold {
        return Some(1.0);
    }
    let r = (t - k.brace_reach - k.brace_hold) / k.brace_release;
    (r < 1.0).then_some(1.0 - crate::active::ease(r))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wall nearer than the reach holds the hand at its face; none (or one beyond) leaves the
    /// reach as it was.
    #[test]
    fn the_brace_meets_the_wall() {
        let shoulder = [0.0, 0.0, 100.0];
        let dir = [0.0, 1.0, 0.0];
        let free = brace_target(shoulder, dir, None, None);
        assert_eq!(brace_target(shoulder, dir, Some(1e4), None), free);
        assert_eq!(brace_target(shoulder, dir, Some(f32::NAN), None), free);
        let near = brace_target(shoulder, dir, Some(20.0), None);
        assert!((near[1] - (20.0 - BRACE_HAND)).abs() < 1e-3, "{near:?}");
        assert_eq!(near[2], free[2]);
        let touching = brace_target(shoulder, dir, Some(1.0), None);
        assert!(touching[1] > 0.0);
    }

    /// A kicked bone swings out and springs back past its pose (an overshoot), then comes to rest.
    #[test]
    fn a_kicked_bone_springs_back_with_an_overshoot() {
        let mut s = Spring::REST;
        s.velocity = [0.0, 0.0, 2.0];
        let mut peak = 0.0f32;
        let mut least = 0.0f32;
        for _ in 0..240 {
            s.step(Role::Spine, 1.0 / 60.0);
            peak = peak.max(s.angle[2]);
            least = least.min(s.angle[2]);
        }
        assert!(peak > 0.05 && peak <= Role::Spine.limit(), "{peak}");
        assert!(least < -0.005, "an overshoot: {least}");
        assert!(s.at_rest(), "{s:?}");
    }

    /// A lowered muscle lets the bone swing farther; a hard kick stops at the joint's limit.
    #[test]
    fn muscles_and_limits() {
        let swing = |dip: f32| {
            let mut s = Spring {
                velocity: [1.0, 0.0, 0.0],
                dip,
                ..Spring::REST
            };
            (0..30)
                .map(|_| {
                    s.step(Role::Spine, 1.0 / 60.0);
                    s.angle[0]
                })
                .fold(0.0f32, f32::max)
        };
        assert!(swing(0.6) > swing(0.0));
        let mut s = Spring {
            velocity: [100.0, 0.0, 0.0],
            ..Spring::REST
        };
        // Held to the joint's speed, it reaches the limit over a few frames and stops there.
        let mut most = 0.0f32;
        for _ in 0..10 {
            s.velocity = [100.0, 0.0, 0.0];
            s.step(Role::Head, 1.0 / 60.0);
            most = most.max(length(s.angle));
            assert!(length(s.angle) <= Role::Head.limit() + 1e-5);
        }
        assert!((most - Role::Head.limit()).abs() < 1e-5);
    }

    /// A pistol round's kick stays near the hit; a .50's or a sledgehammer's spreads through the body.
    #[test]
    fn bigger_hits_spread_farther() {
        let pistol = spread_of(0.3);
        let fifty = spread_of(1.3);
        assert!(fifty > 2.0 * pistol);
        // A hip 0.5 m from a shoulder hit: little from a pistol round, a good share from a .50.
        assert!(weight(Role::Pelvis, 0.5, pistol) < 0.1);
        assert!(weight(Role::Pelvis, 0.5, fifty) > 0.15);
        assert_eq!(spread_of(100.0), SPREAD_MAX);
        assert!(weight(Role::UpperArm, 0.0, pistol) > weight(Role::Pelvis, 0.0, pistol));
    }

    /// The rotation of a rotation vector: a quarter turn about z takes x to y; none is the identity.
    #[test]
    fn the_brace_reaches_holds_and_lets_go() {
        assert_eq!(brace_weight(0.0), Some(0.0));
        assert_eq!(brace_weight(BRACE_REACH + 0.1), Some(1.0));
        let w = brace_weight(BRACE_REACH + BRACE_HOLD + BRACE_RELEASE / 2.0).unwrap();
        assert!((w - 0.5).abs() < 1e-4);
        assert_eq!(
            brace_weight(BRACE_REACH + BRACE_HOLD + BRACE_RELEASE + 0.01),
            None
        );
    }

    #[test]
    fn rotations() {
        let r = rotation([0.0, 0.0, std::f32::consts::FRAC_PI_2]);
        let v = apply_m(&r, [1.0, 0.0, 0.0]);
        assert!((v[0]).abs() < 1e-5 && (v[1] - 1.0).abs() < 1e-5);
        assert_eq!(
            rotation([0.0; 3]),
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        );
        let p = rotation([0.3, 0.2, 0.1]);
        let id = mul(&transpose(&p), &p);
        for (i, x) in id.iter().enumerate() {
            let want = if i % 4 == 0 { 1.0 } else { 0.0 };
            assert!((x - want).abs() < 1e-5);
        }
    }

    /// The role tables are the game's.
    #[test]
    fn the_role_tables() {
        assert_eq!(Role::Pelvis.spring(), (3.2, 0.5));
        assert_eq!(Role::UpperArm.gain(), 0.75);
        assert_eq!(Role::UpperArm.spin(), 5.0);
        assert!(Role::Hand.is_arm() && !Role::Spine.is_arm());
        assert_eq!(Role::Forearm.limit(), 0.7);
        for (i, r) in ROLES.iter().enumerate() {
            assert_eq!(r.index(), i);
        }
    }

    /// No joint turns faster than its role allows; the arms are the slowest and settle without
    /// a long ring.
    #[test]
    fn the_arms_move_like_a_persons() {
        let mut s = Spring {
            velocity: [0.0, 40.0, 0.0],
            ..Spring::REST
        };
        s.step(Role::UpperArm, 1.0 / 60.0);
        assert!(length(s.velocity) <= Role::UpperArm.spin() + 1e-4);
        // Kicked, the arm swings out and is back near its pose within a second, with at most a
        // small overshoot.
        let mut s = Spring {
            velocity: [0.0, 4.0, 0.0],
            ..Spring::REST
        };
        let mut peak = 0.0f32;
        let mut back = 0.0f32;
        for i in 0..120 {
            s.step(Role::UpperArm, 1.0 / 60.0);
            peak = peak.max(s.angle[1]);
            if i > 20 {
                back = back.min(s.angle[1]);
            }
        }
        assert!(peak > 0.2, "it swings: {peak}");
        assert!(
            back > -0.25 * peak,
            "a small follow-through only: {back} of {peak}"
        );
        assert!(length(s.angle) < 0.02, "settled: {:?}", s.angle);
    }

    /// The arms follow a pose with a lag, a little past it, never faster than their bound.
    #[test]
    fn the_arms_follow_with_a_lag() {
        let mut f = Follow::REST;
        let target = [0.0, 0.0, 1.0];
        let mut fastest = 0.0f32;
        let mut most = 0.0f32;
        for i in 0..120 {
            f.step(target, 1.0 / 60.0);
            fastest = fastest.max(length(f.velocity));
            most = most.max(f.angle[2]);
            if i == 2 {
                assert!(f.angle[2] < 0.3, "no jump: {}", f.angle[2]);
            }
        }
        assert!(fastest <= ARM_FOLLOW_SPIN + 1e-4);
        assert!(most > 1.0 && most < 1.15, "a little follow-through: {most}");
        assert!((f.angle[2] - 1.0).abs() < 0.02);
    }

    #[test]
    fn rotation_vectors_go_there_and_back() {
        for w in [
            [0.3, -0.2, 0.9],
            [0.0, 0.0, 0.0],
            [3.0, 0.1, 0.0],
            [0.0, 1.2, -0.4],
        ] {
            let back = rotation_vector(&rotation(w));
            let m1 = rotation(w);
            let m2 = rotation(back);
            for (a, b) in m1.iter().zip(m2.iter()) {
                assert!((a - b).abs() < 1e-3, "{w:?} {back:?}");
            }
        }
    }

    /// Keeping the aim: the arm root takes back the torso's turn, every arm bone keeps a share of
    /// its own; nothing else changes.
    #[test]
    fn the_aim_is_kept() {
        // pelvis <- spine <- clavicle <- upper arm
        let parent = [None, Some(0), Some(1), Some(2)];
        let roles = [Role::Pelvis, Role::Spine, Role::Clavicle, Role::UpperArm];
        let spine = rotation([0.0, 0.0, 0.4]);
        let arm = rotation([0.5, 0.0, 0.0]);
        let mut offsets = vec![IDENTITY, spine, IDENTITY, arm];
        hold_aim(&mut offsets, &parent, &roles, 1.0);
        assert_eq!(offsets[1], spine);
        // The clavicle's world offset (spine x clavicle) is none.
        let world = mul(&spine, &offsets[2]);
        for (a, b) in world.iter().zip(IDENTITY.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
        assert_eq!(offsets[3], IDENTITY);
        let mut half = vec![IDENTITY, spine, IDENTITY, arm];
        hold_aim(&mut half, &parent, &roles, 0.5);
        assert!((length(rotation_vector(&half[3])) - 0.25).abs() < 1e-4);
        let mut none = vec![IDENTITY, spine, IDENTITY, arm];
        hold_aim(&mut none, &parent, &roles, 0.0);
        assert_eq!(none[3], arm);
    }

    /// The arm reaching to a wall behind, ahead, or not at all: the elbow always flexes the way a
    /// person's does (never past straight), and a reach behind never takes the arm further back
    /// than a person's shoulder goes.
    #[test]
    fn arms_reach_as_a_persons() {
        use crate::active::{hinge_angles, HYPEREXTEND};
        let fwd = [0.0, 1.0, 0.0];
        let (s, e0, h0) = ([-13.0, 0.0, 100.0], [-14.0, 0.5, 79.0], [-15.0, 1.0, 59.0]);
        for goal in [
            [-13.0, -31.0, 80.0],
            [-13.0, -40.0, 100.0],
            [-13.0, 31.0, 80.0],
            [-30.0, 0.0, 85.0],
            h0,
        ] {
            let (r1, r2) = reach_arm_ik(s, e0, h0, goal, fwd, 0).expect("reaches");
            let upper = apply_m(&r1, sub(e0, s));
            let fore = apply_m(&mul(&r1, &r2), sub(h0, e0));
            // The elbow's hinge after the reach: the animation's (the body's right, +X) turned
            // with the upper arm.
            let axis = apply_m(&r1, [1.0, 0.0, 0.0]);
            let (flex, _) = hinge_angles(unit(upper).unwrap(), unit(fore).unwrap(), axis);
            assert!(
                flex >= -HYPEREXTEND - 1e-3,
                "{goal:?}: the elbow bent backwards {flex}"
            );
            // Behind: within the shoulder's reach back from straight down (and a hair).
            let back = -dot(upper, fwd);
            if back > 0.0 {
                let down = -upper[2];
                assert!(
                    back.atan2(down) <= SHOULDER_BACK + UPPER_ARM_BACK + 0.02,
                    "{goal:?}: the upper arm went {} back",
                    back.atan2(down)
                );
            }
        }
    }

    /// Pushed backwards or forwards, a stepping leg's knee points forward: it never bends
    /// backwards.
    #[test]
    fn knees_point_forward_whichever_way_the_push() {
        use crate::active::{hinge_angles, HYPEREXTEND};
        let fwd = [0.0, 1.0, 0.0];
        let (h0, k0, a0) = ([-7.0, 0.0, 64.0], [-7.0, 1.0, 35.0], [-7.0, 0.0, 6.0]);
        for (shift, goal) in [
            ([0.0, -12.0, -4.0], [-7.0, -25.0, 6.0]),
            ([0.0, 12.0, -4.0], [-7.0, 25.0, 6.0]),
            ([0.0, -6.0, -10.0], [-7.0, 0.0, 6.0]),
        ] {
            let hip = add(h0, shift);
            let (r1, m, _) = leg_ik(h0, k0, a0, hip, goal, fwd).expect("reaches");
            let thigh = apply_m(&r1, sub(k0, h0));
            let calf = apply_m(&mul(&r1, &m), sub(a0, k0));
            // The knee's hinge: the body's left (-X: a positive turn takes the calf back).
            let axis = apply_m(&r1, [-1.0, 0.0, 0.0]);
            let (flex, _) = hinge_angles(unit(thigh).unwrap(), unit(calf).unwrap(), axis);
            assert!(
                flex >= -HYPEREXTEND - 1e-3,
                "{goal:?}: the knee bent backwards {flex}"
            );
        }
    }
}
