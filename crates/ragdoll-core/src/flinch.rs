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
    (3.0, 0.45),
    (2.6, 0.4),
    (2.2, 0.35),
    (2.4, 0.4),
    (1.6, 0.3),
    (1.8, 0.3),
    (3.0, 0.5),
    (3.2, 0.5),
    (3.4, 0.6),
    (2.0, 0.35),
];
/// How much of a kick each role takes (its lightness).
pub const GAINS: [f32; 11] = [0.35, 0.7, 0.8, 0.9, 1.0, 1.2, 1.1, 0.6, 0.7, 0.3, 1.0];
/// The most each role turns (radians).
pub const LIMITS: [f32; 11] = [0.2, 0.35, 0.4, 0.5, 0.45, 0.9, 0.7, 0.45, 0.6, 0.4, 0.6];

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
        // Semi-implicit Euler, in two halves for the stiff springs.
        for _ in 0..2 {
            let h = dt / 2.0;
            let acc = sub(scale(self.angle, -k), scale(self.velocity, c));
            self.velocity = add(self.velocity, scale(acc, h));
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
pub const BRACE_REACH: f32 = 0.12;
pub const BRACE_HOLD: f32 = 0.5;
pub const BRACE_RELEASE: f32 = 0.4;
pub const FALL_LEAD: f32 = 0.12;
pub const LEDGE_LEAD: f32 = 0.4;
pub const OVERRUN_LEAD: f32 = 0.1;
pub const WINDMILL_HZ: f32 = 2.2;
pub const WINDMILL_SWING: f32 = 1.6;
pub const FALL_LEAN: f32 = 0.4;
pub const FALL_KNEES: f32 = 0.3;
pub const FALL_SHIFT: f32 = 25.0;
pub const BRACE_HOLDS: f32 = 2.5;
/// Bracing on the ground (the host's `crate::knockdown`, the ragdoll's first moments): for this
/// long (s) after the ragdoll takes the body, while the pelvis is more than this high over its
/// lowest bone (units) and dropping faster than this (units a second), the hands and forearms are
/// steered this share a frame toward this speed (m/s along the fall, m/s down).
pub const BRACE_GROUND_SECONDS: f32 = 1.2;
pub const BRACE_GROUND_HEIGHT: f32 = 25.0;
pub const BRACE_GROUND_DROP: f32 = 35.0;
pub const BRACE_GROUND_SHARE: f32 = 0.25;
pub const BRACE_GROUND_SPEED: (f32, f32) = (1.5, 2.5);

/// The brace's weight `t` seconds in: up over the reach, held, down over the release; `None` when
/// it is over.
pub fn brace_weight(t: f32) -> Option<f32> {
    let k = tuning::get();
    if t < 0.0 {
        return Some(0.0);
    }
    if t < k.brace_reach {
        let s = t / k.brace_reach;
        return Some(s * s * (3.0 - 2.0 * s));
    }
    if t < k.brace_reach + k.brace_hold {
        return Some(1.0);
    }
    let r = (t - k.brace_reach - k.brace_hold) / k.brace_release;
    (r < 1.0).then_some(1.0 - r)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        s.step(Role::Head, 1.0 / 60.0);
        assert!((length(s.angle) - Role::Head.limit()).abs() < 1e-5);
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
        assert_eq!(Role::UpperArm.gain(), 1.2);
        assert_eq!(Role::Forearm.limit(), 0.7);
        for (i, r) in ROLES.iter().enumerate() {
            assert_eq!(r.index(), i);
        }
    }
}
