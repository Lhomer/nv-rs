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
pub const BRACE_REACH: f32 = 0.3;
pub const BRACE_HOLD: f32 = 0.5;
pub const BRACE_RELEASE: f32 = 0.5;
pub const FALL_LEAD: f32 = 0.12;
pub const LEDGE_LEAD: f32 = 0.4;
pub const OVERRUN_LEAD: f32 = 0.1;
pub const WINDMILL_HZ: f32 = 1.1;
pub const WINDMILL_SWING: f32 = 1.1;
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
}
