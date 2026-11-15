//! Active ragdoll, stage 2: balance and stepping (Patrick, 2026-11-15), on the stage 1 rig and pose
//! layer ([`crate::active_ragdoll`]). `[nv-rs Gunplay] iBalance` (percent, 0 = off; Settings >
//! Gameplay "Balance"). **Not 1:1 with FalloutNV.exe.**
//!
//! **When it steps.** A hit below the knockdown's topple speed ([`crate::knockdown`]) gives the
//! body's centre of mass (its pelvis) a speed `v = p / mass x iBalance %` (this hit's momentum,
//! the ragdoll's mass, effective weapon stats). Its capture point, where the body would have to
//! stand to stop, lies `v sqrt(h / g)` along the push from the centre of mass ([`capture_offset`],
//! `h` the pelvis's height over the feet). Inside the support (the feet and [`FOOT_REACH`] around
//! them) nothing steps (the flinch alone); outside it, the body takes catch steps toward it
//! ([`plan`]): one or more (at most [`MAX_STEPS`]), each at most [`STEP_LENGTH`] (times its
//! scale), the feet alternating from the one on the push's side, a foot's width apart across the
//! push: bigger hits, more and longer steps.
//!
//! **How it steps.** Real leg movement in the pose layer: each foot is placed by two-bone IK
//! ([`two_bone`]: the thigh and the calf turned so the ankle reaches its target, the knee bending
//! the way it bends now, the foot kept level), the swinging foot along an arc ([`swing`]), the
//! pelvis carried over the feet and a little ahead, the torso twisted and the arms out (raised
//! from the body) while it steps, then the feet plant and the pose blends back to the animation.
//! The actor's position follows the feet only: when a foot lands, the reference and its character
//! controller are set to the feet's middle (no velocity, nothing pushed), the pelvis's offset
//! taking the move back so nothing jumps.
//!
//! **The ground** ([`classify`]): each step's target is put on the navmesh under it (slopes and
//! stairs: the foot lands on the ground, up to [`STEP_UP`] up or down). No walkable ground near
//! it is a wall or an obstacle: the step is shortened (two thirds, then a third), and with no room
//! at all the body catches itself there (stage 3 braces with its hands). Ground far lower is a
//! ledge: the body steps off and falls into its ragdoll ([`crate::knockdown::knock_down`], the
//! fall's damage as the knockdowns count it).
//!
//! **For stage 3 and dismemberment:** [`Limbs`] says which legs and arms the body still has (one
//! leg cannot step: it falls; [`Limbs::legs`]); the plan's outcome ([`Outcome`]) tells stage 3
//! when to brace or fall.

use std::time::Instant;

pub type V3 = [f32; 3];

/// Gravity (m/s2), game units a metre.
pub const GRAVITY: f32 = 9.81;
pub const UNITS_PER_METRE: f32 = 70.0;
/// The support around the feet (m), a step's length at most and the most steps (scale 1), how
/// far apart across the push the feet land (m), a step's time (s) for a full step, the swing's
/// height (m), how far the pelvis leans past the feet (m) and drops while stepping (m).
pub const FOOT_REACH: f32 = 0.15;
pub const STEP_LENGTH: f32 = 0.6;
pub const MAX_STEPS: usize = 4;
pub const STANCE: f32 = 0.12;
pub const STEP_TIME: f32 = 0.3;
pub const MIN_STEP_TIME: f32 = 0.16;
pub const SWING_HEIGHT: f32 = 0.12;
pub const LEAN: f32 = 0.08;
pub const STEP_DROP: f32 = 0.05;
/// The arms out (radians at full), the blend in and out (s), the step's ground: how far across a
/// navmesh point may be from the target (units), how far up or down a foot may land (units), a
/// ledge's drop (units).
pub const ARMS_OUT: f32 = 0.9;
pub const BLEND_IN: f32 = 0.08;
pub const BLEND_OUT: f32 = 0.35;
pub const GROUND_ACROSS: f32 = 30.0;
pub const STEP_UP: f32 = 40.0;
pub const LEDGE: f32 = 60.0;
/// A stepping stops after this whatever it does (s).
pub const MAX_TIME: f32 = 3.0;

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn times(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> Option<V3> {
    let l = length(a);
    (l.is_finite() && l > 1e-6).then(|| times(a, 1.0 / l))
}
fn flat(a: V3) -> V3 {
    [a[0], a[1], 0.0]
}
fn lerp(a: V3, b: V3, t: f32) -> V3 {
    add(a, times(sub(b, a), t))
}

/// Which limbs a body still has (all, until dismemberment takes one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limbs {
    pub left_leg: bool,
    pub right_leg: bool,
    pub left_arm: bool,
    pub right_arm: bool,
}

impl Limbs {
    pub const WHOLE: Limbs = Limbs {
        left_leg: true,
        right_leg: true,
        left_arm: true,
        right_arm: true,
    };

    /// Both legs: it can step.
    pub fn legs(&self) -> bool {
        self.left_leg && self.right_leg
    }

    /// The arm of a side (0 left, 1 right).
    pub fn arm(&self, side: usize) -> bool {
        if side == 0 {
            self.left_arm
        } else {
            self.right_arm
        }
    }
}

/// How far (m) the capture point lies from the centre of mass for a speed `v` (m/s) at a height
/// `h` (m): `v sqrt(h / g)`.
pub fn capture_offset(v: f32, h: f32) -> f32 {
    if !(v > 0.0 && h > 0.0) {
        return 0.0;
    }
    v * (h / GRAVITY).sqrt()
}

/// The catch steps' plan: how far past the support the capture point lies (m), and each step (the
/// foot, 0 left 1 right; its target, game units, on the feet's ground height).
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub need: f32,
    pub steps: Vec<(usize, V3)>,
}

/// The steps that carry the support under the capture point (module documentation): `com` the
/// centre of mass and `feet` the two feet (game units), `dir` the push (unit, flat), `v` (m/s),
/// `h` (m), `scale` the actor's.
pub fn plan(com: V3, feet: [V3; 2], dir: V3, v: f32, h: f32, scale: f32) -> Plan {
    let scale = if scale.is_finite() {
        scale.clamp(0.25, 4.0)
    } else {
        1.0
    };
    let middle = scale_mid(feet);
    let capture = add(
        flat(com),
        times(dir, capture_offset(v, h) * UNITS_PER_METRE),
    );
    // How far the capture point is past the support's edge along the push (m).
    let along = dot(sub(capture, flat(middle)), dir) / UNITS_PER_METRE;
    let need = along - FOOT_REACH * scale;
    if need <= 0.0 {
        return Plan {
            need: need.max(0.0),
            steps: Vec::new(),
        };
    }
    let longest = STEP_LENGTH * scale;
    let count = ((need / longest).ceil() as usize).clamp(1, MAX_STEPS);
    let across = unit(cross([0.0, 0.0, 1.0], dir)).unwrap_or([1.0, 0.0, 0.0]);
    // The first foot: the one on the push's side (ahead along it).
    let first = if dot(sub(feet[0], feet[1]), dir) >= 0.0 {
        0
    } else {
        1
    };
    let mut steps = Vec::with_capacity(count);
    for i in 0..count {
        let foot = (first + i) % 2;
        let reach = need.min(longest * count as f32) * (i + 1) as f32 / count as f32;
        // The feet a foot's width apart across the push, on their own side.
        let side = if dot(sub(feet[foot], middle), across) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let to = add(
            add(
                flat(middle),
                times(dir, (reach + FOOT_REACH * scale) * UNITS_PER_METRE),
            ),
            times(across, side * STANCE * scale * UNITS_PER_METRE),
        );
        steps.push((foot, [to[0], to[1], feet[foot][2]]));
    }
    Plan { need, steps }
}

fn scale_mid(feet: [V3; 2]) -> V3 {
    times(add(feet[0], feet[1]), 0.5)
}

/// What a step's target is on the ground (module documentation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ground {
    /// Walkable: the foot lands at this height.
    Floor(f32),
    /// No walkable ground near it: a wall or an obstacle.
    Blocked,
    /// Ground far lower: a ledge.
    Ledge,
}

/// The ground at a step's target `to` (game units) from the foot's height `from_z`, given the
/// navmesh's nearest walkable point there (none: no navmesh near).
pub fn classify(to: V3, from_z: f32, navmesh: Option<V3>) -> Ground {
    match navmesh {
        Some(at) if length(flat(sub(at, to))) <= GROUND_ACROSS => {
            if at[2] < from_z - LEDGE {
                Ground::Ledge
            } else if (at[2] - from_z).abs() <= STEP_UP {
                Ground::Floor(at[2])
            } else {
                Ground::Blocked
            }
        }
        _ => Ground::Blocked,
    }
}

/// A step's time (s) for a step of `length` (m) at `scale`.
pub fn step_time(length: f32, scale: f32) -> f32 {
    let full = STEP_LENGTH * scale.max(0.25);
    (STEP_TIME * (length / full).clamp(0.0, 1.5).sqrt()).max(MIN_STEP_TIME)
}

/// The swinging foot at `s` (0 to 1) of its step: along the line, lifted in an arc.
pub fn swing(from: V3, to: V3, s: f32, scale: f32) -> V3 {
    let s = s.clamp(0.0, 1.0);
    let eased = s * s * (3.0 - 2.0 * s);
    let lift = SWING_HEIGHT * scale * UNITS_PER_METRE * (std::f32::consts::PI * s).sin();
    add(lerp(from, to, eased), [0.0, 0.0, lift])
}

/// Two-bone IK: the knee for a hip `hip`, a target `target`, the thigh's and the calf's lengths,
/// bending toward `pole` (a point the knee leans to). The target is pulled within reach.
pub fn two_bone(hip: V3, target: V3, thigh: f32, calf: f32, pole: V3) -> Option<(V3, V3)> {
    let to = sub(target, hip);
    let mut d = length(to);
    let u = unit(to)?;
    let reach = (thigh + calf) * 0.999;
    let least = (thigh - calf).abs() * 1.001 + 1e-3;
    d = d.clamp(least, reach);
    let target = add(hip, times(u, d));
    let x = (thigh * thigh - calf * calf + d * d) / (2.0 * d);
    let y = (thigh * thigh - x * x).max(0.0).sqrt();
    let side = sub(pole, hip);
    let n = unit(sub(side, times(u, dot(side, u))))?;
    Some((add(add(hip, times(u, x)), times(n, y)), target))
}

/// The rotation (a rotation vector) that turns direction `a` onto direction `b`.
pub fn between(a: V3, b: V3) -> V3 {
    let (Some(a), Some(b)) = (unit(a), unit(b)) else {
        return [0.0; 3];
    };
    let c = dot(a, b).clamp(-1.0, 1.0);
    let axis = cross(a, b);
    match unit(axis) {
        Some(n) => times(n, c.acos()),
        None => [0.0; 3],
    }
}

/// One planned step under way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub foot: usize,
    pub from: V3,
    pub to: V3,
    pub time: f32,
}

/// How a stepping ended (or will, for stage 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It caught itself.
    Caught,
    /// A wall or an obstacle stopped the steps.
    Wall,
    /// It stepped off a ledge: it falls.
    Ledge,
    /// More steps than it can take: it falls.
    Overrun,
}

/// A body stepping to catch itself (module documentation).
#[derive(Clone, Debug)]
pub struct Stepping {
    /// The push (unit, flat), the speed (m/s) and the attacker (the fall's credit).
    pub dir: V3,
    pub v: f32,
    pub aggressor: u32,
    pub steps: Vec<Step>,
    pub current: usize,
    pub t: f32,
    /// The feet where they stand (world, game units), the IK's weight, the arms' weight, the
    /// pelvis's offset (world, game units) and its speed.
    pub planted: [V3; 2],
    pub weight: f32,
    pub arms: f32,
    pub shift: V3,
    pub shift_v: V3,
    pub recovering: bool,
    pub outcome: Outcome,
    pub since: Instant,
    /// Where the reference is expected (it is moved with the feet only); its AI moving it some
    /// other way ends the stepping. At the start: its height and the feet's middle height.
    pub reference: V3,
    pub reference_z0: f32,
    pub feet_z0: f32,
}

impl Stepping {
    /// A stepping of the plan's steps (their targets already on the ground).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dir: V3,
        v: f32,
        aggressor: u32,
        feet: [V3; 2],
        steps: Vec<(usize, V3)>,
        scale: f32,
        reference: V3,
        outcome: Outcome,
    ) -> Stepping {
        let mut planted = feet;
        let mut list = Vec::new();
        for (foot, to) in steps {
            let from = planted[foot];
            let length = length(flat(sub(to, from))) / UNITS_PER_METRE;
            list.push(Step {
                foot,
                from,
                to,
                time: step_time(length, scale),
            });
            planted[foot] = to;
        }
        Stepping {
            dir,
            v,
            aggressor,
            steps: list,
            current: 0,
            t: 0.0,
            planted: feet,
            weight: 0.0,
            arms: 0.0,
            shift: [0.0; 3],
            shift_v: [0.0; 3],
            recovering: false,
            outcome,
            since: Instant::now(),
            reference,
            reference_z0: reference[2],
            feet_z0: (feet[0][2] + feet[1][2]) / 2.0,
        }
    }

    /// Where the reference goes when a foot lands: the feet's middle across, its height changed
    /// as the feet's middle height changed since the start (stairs, slopes).
    pub fn follow_feet(&self) -> V3 {
        let middle = scale_mid(self.planted);
        [
            middle[0],
            middle[1],
            self.reference_z0 + (middle[2] - self.feet_z0),
        ]
    }

    /// The feet's targets now: each planted foot where it stands, the swinging one on its arc.
    pub fn feet(&self, scale: f32) -> [V3; 2] {
        let mut feet = self.planted;
        if let Some(step) = self.steps.get(self.current).filter(|_| !self.recovering) {
            feet[step.foot] = swing(step.from, step.to, self.t / step.time.max(1e-3), scale);
        }
        feet
    }

    /// One frame of `dt`: the step under way advanced; answers the foot that landed (and where).
    pub fn advance(&mut self, dt: f32) -> Option<(usize, V3)> {
        let dt = dt.clamp(0.0, 0.05);
        if self.recovering {
            self.weight = (self.weight - dt / BLEND_OUT).max(0.0);
            self.arms = (self.arms - dt / BLEND_OUT).max(0.0);
            return None;
        }
        self.weight = (self.weight + dt / BLEND_IN).min(1.0);
        self.arms = (self.arms + dt / BLEND_IN).min(1.0);
        let step = *self.steps.get(self.current)?;
        self.t += dt;
        if self.t < step.time {
            return None;
        }
        self.planted[step.foot] = step.to;
        self.current += 1;
        self.t = 0.0;
        if self.current >= self.steps.len() {
            self.recovering = true;
        }
        Some((step.foot, step.to))
    }

    /// Done: blended back to the animation, or run too long.
    pub fn done(&self) -> bool {
        (self.recovering && self.weight <= 0.0) || self.since.elapsed().as_secs_f32() > MAX_TIME
    }

    /// The pelvis's offset target: over the feet's middle and a little past it along the push
    /// (while stepping), from the animated pelvis `pelvis` (world, game units).
    pub fn shift_target(&self, pelvis: V3, scale: f32) -> V3 {
        if self.recovering {
            return [0.0; 3];
        }
        let middle = scale_mid(self.planted);
        let over = add(
            flat(middle),
            times(self.dir, LEAN * scale * UNITS_PER_METRE),
        );
        let d = sub(over, flat(pelvis));
        [d[0], d[1], -STEP_DROP * scale * UNITS_PER_METRE]
    }

    /// The pelvis's offset moved toward `target` (a critically damped spring, 6 Hz) over `dt`.
    pub fn follow(&mut self, target: V3, dt: f32) {
        let w = std::f32::consts::TAU * 6.0;
        let dt = dt.clamp(0.0, 0.05);
        for _ in 0..2 {
            let h = dt / 2.0;
            let acc = sub(
                times(sub(target, self.shift), w * w),
                times(self.shift_v, 2.0 * w),
            );
            self.shift_v = add(self.shift_v, times(acc, h));
            self.shift = add(self.shift, times(self.shift_v, h));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feet() -> [V3; 2] {
        [[-8.0, 0.0, 0.0], [8.0, 0.0, 0.0]]
    }

    /// The capture point: farther for a faster push and a taller body.
    #[test]
    fn the_capture_point() {
        assert!((capture_offset(1.0, 1.0) - (1.0f32 / GRAVITY).sqrt()).abs() < 1e-6);
        assert!(capture_offset(2.0, 1.0) > capture_offset(1.0, 1.0));
        assert!(capture_offset(1.0, 2.0) > capture_offset(1.0, 1.0));
        assert_eq!(capture_offset(-1.0, 1.0), 0.0);
    }

    /// A light push stays within the feet; a harder one takes a step, a hard one more and longer.
    #[test]
    fn bigger_pushes_take_more_steps() {
        let com = [0.0, 0.0, 70.0];
        let north = [0.0, 1.0, 0.0];
        let light = plan(com, feet(), north, 0.3, 1.0, 1.0);
        assert!(light.steps.is_empty(), "{light:?}");
        let push = plan(com, feet(), north, 1.2, 1.0, 1.0);
        assert_eq!(push.steps.len(), 1, "{push:?}");
        let hard = plan(com, feet(), north, 4.0, 1.0, 1.0);
        assert!(hard.steps.len() >= 2, "{hard:?}");
        // The feet alternate, each lands ahead of the last, across from each other.
        for w in hard.steps.windows(2) {
            assert_ne!(w[0].0, w[1].0);
            assert!(w[1].1[1] > w[0].1[1]);
        }
        let (a, b) = (hard.steps[0].1, hard.steps[1].1);
        assert!((a[0] - b[0]).abs() > 10.0, "a foot's width apart");
        // Never more than the most steps.
        assert!(plan(com, feet(), north, 50.0, 1.0, 1.0).steps.len() <= MAX_STEPS);
    }

    /// The ground: floor within a step up, a wall where no walkable ground is near, a ledge below.
    #[test]
    fn the_ground_at_a_step() {
        let to = [0.0, 40.0, 0.0];
        assert_eq!(
            classify(to, 0.0, Some([5.0, 40.0, 20.0])),
            Ground::Floor(20.0)
        );
        assert_eq!(
            classify(to, 0.0, Some([5.0, 40.0, -30.0])),
            Ground::Floor(-30.0)
        );
        assert_eq!(classify(to, 0.0, Some([5.0, 40.0, -200.0])), Ground::Ledge);
        assert_eq!(classify(to, 0.0, Some([100.0, 40.0, 0.0])), Ground::Blocked);
        assert_eq!(classify(to, 0.0, Some([0.0, 40.0, 90.0])), Ground::Blocked);
        assert_eq!(classify(to, 0.0, None), Ground::Blocked);
    }

    /// Two-bone IK: the knee keeps both lengths and bends toward the pole; out of reach the
    /// target is pulled in.
    #[test]
    fn the_legs_reach() {
        let hip = [0.0, 0.0, 80.0];
        let (knee, target) =
            two_bone(hip, [10.0, 0.0, 10.0], 40.0, 40.0, [0.0, 50.0, 40.0]).unwrap();
        assert!((length(sub(knee, hip)) - 40.0).abs() < 1e-3);
        assert!((length(sub(target, knee)) - 40.0).abs() < 1e-3);
        assert!(knee[1] > 0.0, "toward the pole");
        let (_, far) = two_bone(hip, [0.0, 0.0, -500.0], 40.0, 40.0, [0.0, 50.0, 40.0]).unwrap();
        assert!(length(sub(far, hip)) < 80.0);
        let r = between([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert!((r[2] - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert_eq!(between([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]), [0.0; 3]);
    }

    /// A stepping: the swinging foot arcs from where it stood to its target, lands, the next steps,
    /// and after the last the pose blends back.
    #[test]
    fn a_stepping_steps_then_recovers() {
        let steps = vec![(1, [8.0, 40.0, 0.0]), (0, [-8.0, 70.0, 0.0])];
        let mut s = Stepping::new(
            [0.0, 1.0, 0.0],
            1.5,
            0,
            feet(),
            steps,
            1.0,
            [0.0; 3],
            Outcome::Caught,
        );
        assert_eq!(
            s.feet(1.0)[1],
            feet()[1],
            "at the start the foot is where it stood"
        );
        let mut landed = Vec::new();
        let mut peak = 0.0f32;
        for _ in 0..200 {
            peak = peak.max(s.feet(1.0)[1][2]).max(s.feet(1.0)[0][2]);
            if let Some(l) = s.advance(1.0 / 60.0) {
                landed.push(l);
            }
            if s.done() {
                break;
            }
        }
        assert_eq!(landed.len(), 2);
        assert_eq!(s.planted, [[-8.0, 70.0, 0.0], [8.0, 40.0, 0.0]]);
        assert!(peak > 4.0, "the foot lifts: {peak}");
        assert!(s.done() && s.weight == 0.0);
        let mid = swing([0.0; 3], [0.0, 40.0, 0.0], 0.5, 1.0);
        assert!((mid[1] - 20.0).abs() < 1e-3 && mid[2] > 8.0);
    }

    /// One leg cannot step; an arm lost cannot brace.
    #[test]
    fn limbs() {
        let mut l = Limbs::WHOLE;
        assert!(l.legs() && l.arm(0) && l.arm(1));
        l.left_leg = false;
        l.right_arm = false;
        assert!(!l.legs() && l.arm(0) && !l.arm(1));
    }
}
