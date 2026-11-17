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
/// A brace hand's contact acts on the physical wrist only within this of its rest (m).
pub const CONTACT_NEAR: f32 = 0.1;
/// An arm's share of the body's mass (a brace hand's pin moves it).
pub const ARM_SHARE: f32 = 0.05;
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
/// The chest's, the neck's and the head's motors right after a hit (a share of full; the waist and
/// the limbs keep [`DRIVE_SOFT`]): the struck torso bends instead of the whole body tipping as one
/// piece (Patrick's 126915e clip), ramped back with the rest.
pub const TORSO_SOFT: f32 = 0.1;
/// The neck's and the head's motors right after a hit (a share of full): firmer than the chest's,
/// so a head hit nods the head instead of whipping it (Patrick's 10:47 clip: a .44 at the head
/// whipped it 37 degrees with [`TORSO_SOFT`]).
pub const NECK_SOFT: f32 = 0.55;
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
/// 0.12 s: a 90 to 120 ms rise (Patrick's 19-43-08 clip: 0.04 s threw the arms and head at 12
/// rad/s).
pub const HIT_TIME: f32 = 0.12;
/// The share of a hit's momentum the standing body takes (the rest goes into the ground
/// through the feet): at full, a 0.32 m/s 9mm bent a pistol-armed torso 30 degrees and the
/// bounds held it every frame, a stiff deep bend (Patrick's 19-43-08 clip).
pub const KICK_SHARE: f32 = 0.6;
/// A hit's push reaches each bone after its distance from the impact at this speed (m/s): it
/// travels up and down the body.
pub const HIT_WAVE: f32 = 5.0;
/// A joint turns against its parent at most this many times its role's spin
/// (`crate::flinch::Role::spin`).
pub const SPIN_ROOM: f32 = 1.3;
/// An arm's joint at most its role's spin (the human arms of 2026-11-15: 4 to 6 rad/s), the speed
/// saturating smoothly from [`SPIN_KNEE`] of it.
pub const ARM_SPIN_ROOM: f32 = 1.1;
/// An unarmed body's arms against [`ARM_SPIN_ROOM`] (a share of it): no weapon pose holds them,
/// and a hit flung them (Patrick's 10:47 clip: 9 to 20 rad/s, through the body).
pub const UNARMED_ARM_SPIN: f32 = 0.55;
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
pub const HYPEREXTEND: f32 = 0.02;
pub const ELBOW_LEAN: f32 = 0.3;
pub const KNEE_LEAN: f32 = 0.15;
pub const AXIS_BEND: f32 = 0.2;
/// The least flex (rad) an elbow's or a knee's target keeps ([`Body::hinged`]).
pub const TARGET_FLEX: f32 = 0.06;
/// Counter-side bracing ([`Body::hit`]): how much a hit (per m/s of its push) softens the bones on
/// its side and stiffens those away from it (as dips).
pub const COUNTER_SOFT: f32 = 0.1;
pub const COUNTER_STIFF: f32 = 0.2;
/// How much firmer a hinge's motor is near and past its stop (as a negative dip).
pub const HINGE_FIRM: f32 = 4.0;
/// How near its stop (rad) a hinge's motor is made firmer.
pub const HINGE_NEAR: f32 = 0.08;
/// The share of the way back into its range a hinge past it is turned each small step.
pub const HINGE_SHARE: f32 = 0.05;
/// The same for a knee.
pub const KNEE_SHARE: f32 = 0.05;
/// How short of its straight length a leg's foot pin is held (a share of it).
pub const REACH_SHORT: f32 = 0.995;

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

/// How far a world turn `r` turns the flat heading `facing` (rad, signed, -pi to pi): the yaw of a
/// shown pelvis against its animation (the host's pose log and the lab's `yaw_rate`: a body that
/// spins shows it).
pub fn heading_off(r: &M3, facing: V3) -> f32 {
    let f = [facing[0], facing[1], 0.0];
    let g = mv(r, f);
    let (a, b) = (f[1].atan2(f[0]), g[1].atan2(g[0]));
    let mut d = b - a;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

/// The turn (rad) between two yaws (`heading_off`), the short way round.
pub fn yaw_between(a: f32, b: f32) -> f32 {
    let d = (b - a).abs() % std::f32::consts::TAU;
    d.min(std::f32::consts::TAU - d)
}

/// How long one hit's reaction holds the body (s from the last hit) when it does not topple: then
/// it is handed back to the animation (eased out over [`BLEND_BACK`]) whatever still holds it (a
/// brace's hands let go and are handed back too: `crate::brace::let_go`). Patrick's c2a53c0 fold
/// clip: one .44 hit held its body 5.7 s, the actor's AI walking and turning it all the while.
pub const REACTION_MOST: f32 = 2.0;

/// The physics' weight in the shown pose (as [`weight`]) `since_busy` seconds after the reaction
/// was last busy (steps, a brace, a fall) and `since_hit` after its last hit: a reaction that is not
/// going over ([`REACTION_MOST`]) starts handing back by then, busy or not.
pub fn reaction_weight(
    since_busy: f32,
    since_hit: f32,
    falling: bool,
    since_yield: Option<f32>,
) -> f32 {
    let since = if falling {
        since_busy
    } else {
        since_busy.max(since_hit - REACTION_MOST + tuning::get().active_for)
    };
    let w = weight(since);
    match since_yield {
        Some(y) if !falling => w.min(1.0 - ease(y / YIELD_BACK)),
        _ => w,
    }
}

/// How fast a reaction yields to its animation once the animation moves on (the actor's AI
/// walks or turns it, or its animation jumps: a new set after a cripple): its body handed back
/// over this (s, eased), never fighting the animation's own motion (Patrick's 14-16-02 clip: a
/// 0.53 m/s flinch on an actor starting to walk and turn went to 48 units off and spun).
pub const YIELD_BACK: f32 = 0.35;
/// The AI's move of the actor (game units) or turn (rad) since the last hit past which the
/// reaction yields ([`YIELD_BACK`]).
pub const YIELD_MOVE: f32 = 6.0;
pub const YIELD_TURN: f32 = 0.25;

/// The farthest a drawn bone may be from its animated place (game units, [`too_far`]): past it the
/// body is dropped and nothing is written that frame (a Young Bighorner was drawn 1047 units off
/// for a frame: Patrick's 21-16-09 clip).
pub const MOST_DRAWN_OFF: f32 = 30.0;

/// The bone drawn farthest past [`MOST_DRAWN_OFF`] from its animated place (`shown` against
/// `anim`, world joints, game units; `checked` says which count), and how far.
pub fn too_far(shown: &[V3], anim: &[V3], checked: impl Fn(usize) -> bool) -> Option<(usize, f32)> {
    shown
        .iter()
        .zip(anim)
        .enumerate()
        .filter(|(i, _)| checked(*i))
        .map(|(i, (s, a))| (i, length(sub(*s, *a))))
        .filter(|(_, d)| d.is_nan() || *d > MOST_DRAWN_OFF)
        .fold(None, |best: Option<(usize, f32)>, (i, d)| match best {
            Some((_, b)) if b >= d => best,
            _ => Some((i, d)),
        })
}

/// How soon after a hit (s) a jump of the animation confined to the legs is re-based instead of
/// yielded to ([`rebases`]).
pub const REBASE_WINDOW: f32 = 0.2;

/// A leg bone's animated local turning more than this in a frame (rad) is a jump of its
/// animation ([`leg_jumped`]), whatever the frame rate: 117 to 155 tolerances (about 0.25 to 0.3
/// rad) were flagged at Patrick's frame rate, under `pose::capped`'s 0.35 at 60.
pub const LEG_JUMP_TURN: f32 = 0.15;

/// Whether a leg bone's animated local jumped from `prev` to `next` ([`LEG_JUMP_TURN`]).
pub fn leg_jumped(prev: &(M3, V3), next: &(M3, V3)) -> bool {
    let d = qnorm(qmul(qfrom_m(&next.0), qconj(qfrom_m(&prev.0))));
    length(qlog(short(d))) > LEG_JUMP_TURN
}

/// Whether the animation's jump `since_hit` seconds after the last hit is re-based (the body's
/// world pose kept, its offsets put against the new animation: nothing drawn jumps) rather than
/// yielded to: a leg jumped (`leg_jump`, [`leg_jumped`]) and every bone the animation jumped by
/// more than its frame's share (`cut`, `pose::capped`: their roles) is a leg. A game's hit
/// animation starting under the reaction jumps the legs a frame or two after the hit (Patrick's
/// 21-16-09 clip: 7 of 27 hits, always the L Calf, 117 to 155 tolerances); carried into the body
/// and yielded to, the upper body popped up and sideways for 1 to 3 frames and the flinch was
/// gone.
pub fn rebases(since_hit: f32, leg_jump: bool, cut: &[Role]) -> bool {
    since_hit <= REBASE_WINDOW && leg_jump && cut.iter().all(|r| r.is_leg())
}

/// How fast the AI moves (game units/s) or turns (rad/s) an actor walking or turning on its own:
/// a push below a knock-down then takes no catch steps, its stride carries it (steps planned from
/// a stride and followed while the AI walks it threw the pelvis 7 units sideways in the lab).
pub const WALKING_SPEED: f32 = 15.0;
pub const TURNING_SPEED: f32 = 0.6;

/// Whether the AI is walking or turning the actor itself ([`WALKING_SPEED`], [`TURNING_SPEED`]).
pub fn ai_walking(speed: f32, turn_rate: f32) -> bool {
    speed >= WALKING_SPEED || turn_rate.abs() >= TURNING_SPEED
}

/// Whether a reaction yields now: the AI moved the actor `moved` units or turned it `turned` rad
/// since the last hit (an animation jump yields at once: its caller's).
pub fn yields(moved: f32, turned: f32) -> bool {
    moved >= YIELD_MOVE || turned >= YIELD_TURN
}

/// The most a reaction's offset may be for its push `v` (m/s: the most of its hits'): each bone's
/// world turn off its animation (rad: 8 degrees at 0.3 m/s, 20 at 0.75, along a line between, at
/// least [`TURN_LEAST`] and at most 75 degrees; Patrick's 19-43-08 clip: a 0.32 m/s 9mm turned the
/// torso 20 to 27 degrees), its yaw about the vertical (rad, never more than the turn), and the
/// pelvis's offset (game units; the body's, not the steps'). A 0.53 m/s flinch: 14 degrees, 6.5
/// degrees of yaw, 6.2 units.
pub fn offset_most(v: f32) -> (f32, f32, f32) {
    let v = v.clamp(0.0, 3.0);
    let turn = (TURN_AT_SLOW + (v - SLOW) * (TURN_AT_FAST - TURN_AT_SLOW) / (FAST - SLOW))
        .clamp(TURN_LEAST, 75.0)
        .to_radians();
    (turn, (0.06 + 0.1 * v).min(0.3).min(turn), 2.0 + 8.0 * v)
}

/// [`offset_most`]'s turn: [`TURN_AT_SLOW`] degrees at [`SLOW`] m/s, [`TURN_AT_FAST`] at [`FAST`],
/// never under [`TURN_LEAST`].
pub const SLOW: f32 = 0.3;
pub const FAST: f32 = 0.75;
pub const TURN_AT_SLOW: f32 = 8.0;
pub const TURN_AT_FAST: f32 = 20.0;
pub const TURN_LEAST: f32 = 4.0;

/// The most the shown pelvis may drop under its animated height (game units): a flinch, and
/// while catch steps carry it. Patrick's 19-43-08 clip (0c1fbbf): a .44's step sank the Fiend
/// upright, hips below the ground.
pub const PELVIS_DROP_FLINCH: f32 = 4.0;
pub const PELVIS_DROP_STEP: f32 = 8.0;

/// The shown pelvis's offset from its animation `offset` (game units) held up to
/// [`PELVIS_DROP_FLINCH`] under it, [`PELVIS_DROP_STEP`] while `stepping` (never lifted: only a
/// drop is held).
pub fn hold_pelvis_up(offset: V3, stepping: bool) -> V3 {
    let most = if stepping {
        PELVIS_DROP_STEP
    } else {
        PELVIS_DROP_FLINCH
    };
    [offset[0], offset[1], offset[2].max(-most)]
}

/// Each foot's target `feet` (world, game units) never under its animated place `animated`
/// (the animation's foot is on the floor) nor under its floor plus its ankle `floor` where the
/// collision found one: a planted foot is never put into the floor (Patrick's 19-43-08 clip).
pub fn feet_on_floor(feet: [V3; 2], animated: [V3; 2], floor: [Option<f32>; 2]) -> [V3; 2] {
    [0, 1].map(|i| {
        let low = floor[i].map_or(animated[i][2], |f| f.max(animated[i][2]));
        [feet[i][0], feet[i][1], feet[i][2].max(low)]
    })
}

/// How fast a shown foot's lift grows or shrinks (game units a second, [`lift_amount`]): never in
/// a frame (Patrick's 21-16-09 clip).
pub const LIFT_RISE: f32 = 40.0;
/// How far under its animated ankle a shown foot may be before it is lifted (game units,
/// [`lift_amount`]): the lift then only takes the rest, so it starts from nothing. Lifting a nearly
/// straight leg's ankle a unit bends its knee out about 5: followed exactly, an animated foot
/// rising a unit (a hit animation starting) swung the knee 4 units in a frame.
pub const LIFT_SLACK: f32 = 1.5;

/// How much a shown foot is lifted now ([`lift_feet`]: game units): toward `excess` (how far it
/// is shown under its animated ankle less [`LIFT_SLACK`], or under its collision floor plus its
/// ankle; 0 while catch steps place the legs) from `was`, last frame's, by at most [`LIFT_RISE`]
/// over `dt` either way: a lift starting, a step's end or start, an animated foot jumping up
/// (the game's hit animation starting) are followed, never jumped to in a frame.
pub fn lift_amount(was: f32, excess: f32, dt: f32) -> f32 {
    let r = LIFT_RISE * dt.max(0.0);
    let was = if was.is_finite() { was.max(0.0) } else { 0.0 };
    excess.max(0.0).clamp((was - r).max(0.0), was + r)
}

/// The shown legs lifted off the floor: each leg (`legs`: thigh, calf and foot bones) whose ankle
/// is shown (`at`: the shown joints, world, game units) under `low` (its animated ankle's
/// height, or its collision floor plus its ankle where higher) solved as two bones from its hip
/// so the ankle is at `low`, where it is across, its knee kept in the plane it is bent in (toward
/// `forward` for a straight leg); the shown world rotations `shown` of its thigh and calf turned
/// the short way onto it (the foot keeps its own). A planted foot never sinks into the floor under
/// the body's lowered pelvis (Patrick's 19-43-08 clip), and a small lift is a small move: solved
/// against a fixed pole, a lift of a unit swung the knee 6 units (21-16-09). Answers how many
/// legs were lifted.
pub fn lift_feet(
    shown: &mut [M3],
    at: &[V3],
    legs: &[(usize, usize, usize)],
    low: &[f32],
    forward: V3,
) -> u32 {
    use crate::balance::{between, two_bone};
    use crate::flinch::{apply_m, mul, rotation};
    let mut lifted = 0;
    for (&(t, c, f), &low) in legs.iter().zip(low) {
        let (hip, knee, ankle) = (at[t], at[c], at[f]);
        if ankle[2] >= low - 0.01 {
            continue;
        }
        let (l1, l2) = (length(sub(knee, hip)), length(sub(ankle, knee)));
        if l1 < 1.0 || l2 < 1.0 {
            continue;
        }
        let goal = [ankle[0], ankle[1], low];
        // The knee's side of the hip-ankle line as it is, leaning forward the straighter the leg
        // (no switch between the two: the knee never jumps sides).
        let off = sub(knee, scale(add(hip, ankle), 0.5));
        let side = add(off, scale([forward[0], forward[1], 0.0], 0.5));
        let Some(side) = unit(side) else {
            continue;
        };
        let Some((k, reached)) = two_bone(hip, goal, l1, l2, add(knee, scale(side, 20.0))) else {
            continue;
        };
        let r1 = rotation(between(sub(knee, hip), sub(k, hip)));
        let r2 = rotation(between(apply_m(&r1, sub(ankle, knee)), sub(reached, k)));
        shown[t] = mul(&r1, &shown[t]);
        shown[c] = mul(&mul(&r2, &r1), &shown[c]);
        lifted += 1;
    }
    lifted
}

/// The fastest a bone's shown offset from its animation may change (rad/s): a person's arm, 4 to 6
/// rad/s (Patrick's 19-43-08 clip: the arms and head at 12 under a bound of 20; the 14-16-02
/// clip's upper body turned 3 to 4 times round in under a second).
pub const SPIN_MOST: f32 = 6.0;
/// A bone's own motion about the pelvis handed over to the ragdoll at most this times its
/// distance (rad/s, [`handover_velocities`]).
pub const HANDOVER_SPIN: f32 = 20.0;
/// How fast a bound pulls an offset beyond it back (rad/s, game units/s): an offset the bound
/// meets out past it (a fall that did not happen ending) is brought in, never cut in a frame.
pub const PULL_TURN: f32 = 3.0;
pub const PULL_MOVE: f32 = 40.0;

/// The yaw of a world turn `q` about the vertical (rad, -pi to pi): its swing-twist twist.
pub fn yaw_of(q: Q) -> f32 {
    let q = short(q);
    2.0 * q[3].atan2(q[0])
}

/// A reaction's shown offsets held to what its push justifies ([`offset_most`]) and to
/// [`SPIN_MOST`] a second, each bone's offset kept from the frame before on the hemisphere of
/// its animation (the shorter way round).
#[derive(Clone, Debug, Default)]
pub struct Limit {
    before: Vec<Q>,
    pelvis: Option<V3>,
    /// Each bone held a frame before and not caught up with its offset yet: it catches up at
    /// twice the pull ([`PULL_TURN`]), never at the spin bound (it jumped at 20 rad/s as a bound
    /// let go in the lab).
    catching: Vec<bool>,
}

impl Limit {
    /// Bone `i`'s kept offset moved onto a new animation (its animated world rotation `from` to
    /// `to`): the world rotation it held stays, so the new animation's jump is not drawn
    /// ([`rebases`]).
    pub fn rebase(&mut self, i: usize, from: Q, to: Q) {
        if let Some(b) = self.before.get_mut(i) {
            *b = short(qnorm(qmul(qmul(*b, from), qconj(to))));
        }
    }

    /// Nothing kept (a new body).
    pub fn open(&mut self) {
        self.before.clear();
        self.pelvis = None;
        self.catching.clear();
    }

    /// The shown offsets only followed, not held (a fall: the ragdoll's), so the bounds pull in
    /// from where it was when they hold again.
    pub fn follow(&mut self, shown: &[Q], anim: &[Q], pelvis: V3) {
        self.before = shown
            .iter()
            .zip(anim)
            .map(|(s, a)| short(qnorm(qmul(*s, qconj(*a)))))
            .collect();
        self.pelvis = Some(pelvis);
    }

    /// The shown world rotations `shown` over their animated `anim` and the pelvis's offset
    /// `pelvis` (the body's, game units) held for a push `v` (m/s), `dt` on; not the bones `free`
    /// says (their bounds, the spin's too). Answers how many were held.
    pub fn hold(
        &mut self,
        shown: &mut [Q],
        anim: &[Q],
        pelvis: &mut V3,
        v: f32,
        dt: f32,
        free: impl Fn(usize) -> bool,
    ) -> u32 {
        let (angle_most, yaw_most, pelvis_most) = offset_most(v);
        let dt = dt.max(1e-4);
        let step_most = SPIN_MOST * dt;
        if self.before.len() != shown.len() {
            self.before = shown
                .iter()
                .zip(anim)
                .map(|(s, a)| short(qnorm(qmul(*s, qconj(*a)))))
                .collect();
        }
        if self.catching.len() != shown.len() {
            self.catching = vec![false; shown.len()];
        }
        let mut held = 0;
        for (i, (s, a)) in shown.iter_mut().zip(anim).enumerate() {
            let mut off = short(qnorm(qmul(*s, qconj(*a))));
            if free(i) {
                self.before[i] = off;
                self.catching[i] = false;
                continue;
            }
            let was = off;
            // Its yaw about the vertical, then its whole turn.
            let yaw = yaw_of(off);
            if yaw.abs() > yaw_most {
                let back = qexp([0.0, 0.0, yaw.signum() * yaw_most - yaw]);
                off = qnorm(qmul(back, off));
            }
            let w = qlog(off);
            let l = length(w);
            if l > angle_most {
                off = qexp(scale(w, angle_most / l));
            }
            // No faster than the spin bound from the frame before; a bound pulling it in, no
            // faster than its pull.
            let bound = length(qlog(qmul(off, qconj(was)))) > 1e-5;
            let most = if bound {
                PULL_TURN * dt
            } else if self.catching[i] {
                2.0 * PULL_TURN * dt
            } else {
                step_most
            };
            // Coming back toward its animation is never held to the pull: as much as the offset
            // shrinks, up to the spin bound (the hand-back eases it to nothing; held at the pull,
            // its last frame jumped: Patrick's 19-43-08 clip, 42.6 degrees for one frame, then
            // none). `was`: the offset as the body and its weight have it, before the bounds.
            let shrink = length(qlog(self.before[i])) - length(qlog(was));
            let most = most.max(shrink.min(step_most));
            let d = qlog(qmul(off, qconj(self.before[i])));
            let l = length(d);
            if l > most {
                off = qnorm(qmul(qexp(scale(d, most / l)), self.before[i]));
            }
            // Held, or not caught up yet: it catches up gently.
            self.catching[i] = bound || (self.catching[i] && l > most);
            if length(qlog(qmul(off, qconj(was)))) > 1e-5 {
                held += 1;
                *s = qnorm(qmul(off, *a));
            }
            self.before[i] = short(off);
        }
        let l = length(*pelvis);
        if l > pelvis_most {
            let mut to = scale(*pelvis, pelvis_most / l);
            // Pulled in from where it was, no faster than the pull.
            if let Some(was) = self.pelvis {
                let d = sub(to, was);
                let dl = length(d);
                let most = PULL_MOVE * dt;
                if dl > most && length(was) > pelvis_most {
                    to = add(was, scale(d, most / dl));
                }
            }
            *pelvis = to;
            held += 1;
        }
        self.pelvis = Some(*pelvis);
        held
    }
}

/// The velocities (m/s) the ragdoll takes over from the body (`host::knockdown`'s launch): the
/// pelvis's held to `most_speed` (the hit's speed, a fall's), and each bone's own motion about the
/// pelvis to [`HANDOVER_SPIN`] times its distance from it. A body's arm spinning at 61 to 169 rad/s
/// before the switch flung the ragdoll about in the air (Patrick's 18-21-05 clip).
pub fn handover_velocities(state: &[State], pelvis: usize, most_speed: f32) -> Vec<V3> {
    let Some(p) = state.get(pelvis) else {
        return state.iter().map(|s| s.v).collect();
    };
    let most = if most_speed.is_finite() {
        most_speed.max(0.0)
    } else {
        0.0
    };
    let pv = p.v;
    let l = length(pv);
    let held = if l > most && l > 0.0 {
        scale(pv, most / l)
    } else {
        pv
    };
    state
        .iter()
        .map(|s| {
            let own = sub(s.v, pv);
            let r = length(sub(s.com, p.com));
            let cap = HANDOVER_SPIN * r;
            let ol = length(own);
            let own = if ol > cap && ol > 0.0 {
                scale(own, cap / ol)
            } else {
                own
            };
            let v = add(held, own);
            if v.iter().all(|x| x.is_finite()) {
                v
            } else {
                held
            }
        })
        .collect()
}

/// `q` on the hemisphere of the identity (the shorter way round).
fn short(q: Q) -> Q {
    if q[0] < 0.0 {
        q.map(|x| -x)
    } else {
        q
    }
}

/// A reaction being handed back only settles toward its animation: while the body's weight goes
/// down (no hit since, not going over), each bone's shown world rotation may only come closer to
/// its target, and the shown pelvis to its own; a body that runs off on its own (a pin, a turn,
/// the AI walking the actor) is never shown gaining on it (Patrick's c2a53c0 fold clip: the pelvis
/// 3.5 to 23 to 32 units after the hands let go, no hit).
#[derive(Clone, Debug, Default)]
pub struct Settle {
    /// Each bone's offset angle (rad) and the pelvis's (game units) the frame before; none while
    /// not settling.
    angle: Vec<f32>,
    pelvis: Option<f32>,
}

impl Settle {
    /// Not settling (the reaction is on): nothing held.
    pub fn open(&mut self) {
        self.angle.clear();
        self.pelvis = None;
    }

    /// Whether it is holding (settling).
    pub fn holding(&self) -> bool {
        self.pelvis.is_some()
    }

    /// The shown world rotations `shown` over their `target`s and the shown pelvis's offset from
    /// its own target `pelvis` (game units), held to no more than the frame before's; not the
    /// bones `free` says (a brace's arms, on their own way home: held, they spun).
    pub fn hold(
        &mut self,
        shown: &mut [Q],
        target: &[Q],
        pelvis: &mut V3,
        free: impl Fn(usize) -> bool,
    ) {
        self.hold_down(shown, target, pelvis, 1.0, free)
    }

    /// As [`Settle::hold`], each ceiling first brought down by `keep` (the body's weight over the
    /// frame before's): it settles with the hand-back, never frozen while it turns (Patrick's
    /// 14-16-02 clip held the pelvis at 34.31 units for 0.25 s).
    pub fn hold_down(
        &mut self,
        shown: &mut [Q],
        target: &[Q],
        pelvis: &mut V3,
        keep: f32,
        free: impl Fn(usize) -> bool,
    ) {
        let keep = keep.clamp(0.0, 1.0);
        for most in self.angle.iter_mut() {
            if most.is_finite() {
                *most *= keep;
            }
        }
        if let Some(p) = self.pelvis.as_mut() {
            *p *= keep;
        }
        if self.angle.len() != shown.len() {
            self.angle = vec![f32::INFINITY; shown.len()];
        }
        for (i, ((s, t), most)) in shown
            .iter_mut()
            .zip(target)
            .zip(self.angle.iter_mut())
            .enumerate()
        {
            if free(i) {
                *most = f32::INFINITY;
                continue;
            }
            let v = qlog(qmul(*s, qconj(*t)));
            let a = length(v);
            if a > *most && a > 1e-6 {
                *s = qnorm(qmul(qexp(scale(v, *most / a)), *t));
            } else {
                *most = a;
            }
        }
        let l = length(*pelvis);
        match self.pelvis {
            Some(most) if l > most && l > 1e-6 => *pelvis = scale(*pelvis, most / l),
            _ => self.pelvis = Some(l),
        }
    }
}

/// The torso's motors at the limbs' `strength` ([`strength`]): from [`TORSO_SOFT`] instead of
/// [`DRIVE_SOFT`], back to full as they come back.
pub fn torso_strength(strength: f32) -> f32 {
    soft_strength(strength, tuning::get().torso_soft)
}

/// A motor softer than the limbs' right after a hit: from `soft` instead of [`DRIVE_SOFT`] at the
/// limbs' `strength`, back to full as they come back.
pub fn soft_strength(strength: f32, soft: f32) -> f32 {
    let t = tuning::get();
    let back = ((strength - t.drive_soft) / (1.0 - t.drive_soft).max(1e-3)).clamp(0.0, 1.0);
    soft + (1.0 - soft) * back
}

/// The motors' strength `since` seconds after the last hit (soft, eased back to full).
pub fn strength(since: f32) -> f32 {
    let t = tuning::get();
    t.drive_soft + (1.0 - t.drive_soft) * ease(since / t.drive_ramp.max(0.05))
}

// --- The rig's physics parents. --------------------------------------------------------------------

/// Each rig bone's parent in the physical body, from its parent in the node tree (`node`: the rig
/// bone above it, `None` when it reaches none) and the pelvis's index: a bone that reaches no rig
/// bone hangs from the pelvis. FNV's `skeleton.nif` puts `Bip01 Spine` under `Bip01 NonAccum`, a
/// sibling of `Bip01 Pelvis` (the thighs are under the pelvis): read as the node tree, the whole
/// upper body (spine to head and the arms, about 55% of the mass) was a second free root with no
/// joint, motor, carry or limit to the pelvis, and a hit spun it as one stiff plank until the body
/// was dropped (Patrick's 126915e clip: the slow lean back). The node tree is still what the
/// write follows; only the body uses this.
pub fn physics_parents(node: &[Option<usize>], pelvis: usize) -> Vec<Option<usize>> {
    node.iter()
        .enumerate()
        .map(|(b, p)| match p {
            Some(p) => Some(*p),
            None if b == pelvis => None,
            None => Some(pelvis),
        })
        .collect()
}

// --- The way back from a dropped body. --------------------------------------------------------------

/// How long a body dropped as not believable takes back to the animation (s): the shown pose's
/// last offsets over the animation eased out ([`Fade`]), never written in one frame.
pub const FADE_BACK: f32 = 0.3;

/// A dropped body's way back to its animation: each bone's last shown world rotation over its
/// animated one and the pelvis's last offset (game units), eased out over [`FADE_BACK`]. Laid on
/// whatever is shown after (the animation, or a new reaction's pose), so nothing jumps.
#[derive(Clone, Debug)]
pub struct Fade {
    off: Vec<Q>,
    pelvis: V3,
    t: f32,
}

impl Fade {
    /// From the last shown pose's offsets (`offsets`) and pelvis offset.
    pub fn new(off: Vec<Q>, pelvis: V3) -> Fade {
        Fade {
            off,
            pelvis,
            t: 0.0,
        }
    }

    /// Each bone's shown world rotation over its animated one (`S A^T`).
    pub fn offsets(shown: &[M3], anim: &[M3]) -> Vec<Q> {
        shown
            .iter()
            .zip(anim)
            .map(|(s, a)| {
                let q = qnorm(qmul(qfrom_m(s), qconj(qfrom_m(a))));
                // The short way round.
                if q[0] < 0.0 {
                    q.map(|x| -x)
                } else {
                    q
                }
            })
            .collect()
    }

    /// `dt` on; false once it is over.
    pub fn advance(&mut self, dt: f32) -> bool {
        self.t += dt.max(0.0);
        self.t < FADE_BACK
    }

    /// How much of the last pose is left (1 at the drop, eased to 0).
    pub fn left(&self) -> f32 {
        1.0 - ease(self.t / FADE_BACK)
    }

    /// What is left laid on a shown pose (world rotations) and its pelvis offset.
    pub fn on(&self, shown: &mut [M3], pelvis: &mut V3) {
        let k = self.left();
        for (s, off) in shown.iter_mut().zip(&self.off) {
            let r = qslerp(Q_ID, *off, k);
            *s = m_from_q(qnorm(qmul(r, qfrom_m(s))));
        }
        *pelvis = add(*pelvis, scale(self.pelvis, k));
    }
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
    /// Per hand pin (empty: all 1): its share of the pin's stiffness (`crate::brace::Hand::pin`).
    pub hand_share: Vec<f32>,
    /// Per hand pin (empty or zero: a plain pin): a contact's way out of its surface (unit): the
    /// pin then only pushes (the wrist pressed into the surface is pushed out and held from
    /// sliding; one off it is left free).
    pub hand_normal: Vec<V3>,
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
    /// The forearms and hands against the torso ([`Body::keep_limbs_out`]): each pair (limb bone,
    /// torso bone) and how deep the limb's capsule may be in the torso's (m): as deep as the pose
    /// it was made from had it, no deeper.
    pub outside: Vec<(usize, usize, f32)>,
    /// Whether [`Body::keep_limbs_out`] runs (an unarmed body; an armed one's arms are the weapon
    /// pose's and its aim's).
    pub limbs_out: bool,
    /// The arms' spin bound against the tuning's (`arm_spin_room`): [`UNARMED_ARM_SPIN`] for an
    /// unarmed body that neither braces nor falls ([`Body::unarmed`]), 1 otherwise.
    pub arm_spin: f32,
}

/// The closest points of the segments `p1`-`q1` and `p2`-`q2`.
pub fn closest_points(p1: V3, q1: V3, p2: V3, q2: V3) -> (V3, V3) {
    let (d1, d2, r) = (sub(q1, p1), sub(q2, p2), sub(p1, p2));
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
    let (s, t) = if a <= 1e-12 && e <= 1e-12 {
        (0.0, 0.0)
    } else if a <= 1e-12 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = dot(d1, r);
        if e <= 1e-12 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = dot(d1, d2);
            let den = a * e - b * b;
            let s0 = if den > 1e-12 {
                ((b * f - c * e) / den).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                ((-c / a).clamp(0.0, 1.0), 0.0)
            } else if t0 > 1.0 {
                (((b - c) / a).clamp(0.0, 1.0), 1.0)
            } else {
                (s0, t0)
            }
        }
    };
    (add(p1, scale(d1, s)), add(p2, scale(d2, t)))
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
        let mut body = Body {
            segments,
            state,
            pelvis,
            mass,
            dip: vec![0.0; n],
            pushes: Vec::new(),
            hinge: vec![None; n],
            outside: Vec::new(),
            limbs_out: true,
            arm_spin: 1.0,
        };
        for l in 0..n {
            if !matches!(body.segments[l].role, Role::Forearm | Role::Hand) {
                continue;
            }
            for t in 0..n {
                if matches!(body.segments[t].role, Role::Pelvis | Role::Spine) {
                    let deep = body.depth(l, t).0.max(0.0);
                    body.outside.push((l, t, deep));
                }
            }
        }
        body
    }

    /// The shown pose's upper arms held inside the shoulder's human range
    /// (`crate::flinch::shoulder_range`) for a chest facing `fwd` with its up `up`: an arm past it
    /// is turned about its shoulder, the least way back in, its forearm and hand with it (their
    /// joints moved too). An arm the animation itself holds past the range (a rifle's support
    /// arm across the chest) may be as far past it as the animation has it (`anim`, its chest
    /// turned as the shown one is not: the animation's own forward and up, `anim_fwd`), no
    /// further. The side of each arm is told by where its shoulder is. The hands in `skip` (an
    /// arm a brace or a grip puts on its goal: its goal is held already) are left as they are.
    #[allow(clippy::too_many_arguments)]
    pub fn hold_shoulders(
        &self,
        rots: &mut [M3],
        joints: &mut [V3],
        fwd: V3,
        up: V3,
        anim: &[(M3, V3)],
        anim_fwd: V3,
        skip: &[usize],
    ) {
        let n = self.segments.len();
        if rots.len() != n || joints.len() != n || anim.len() != n {
            return;
        }
        let right = cross(fwd, up);
        for h in 0..n {
            if self.segments[h].role != Role::Hand || skip.contains(&h) {
                continue;
            }
            let Some(f) = self.segments[h].parent else {
                continue;
            };
            let Some(u) = self.segments[f].parent else {
                continue;
            };
            let Some(c) = self.segments[u].parent else {
                continue;
            };
            let side = if dot(sub(joints[u], joints[c]), right) >= 0.0 {
                1
            } else {
                0
            };
            let (shoulder, elbow) = (joints[u], joints[f]);
            let allowed =
                crate::flinch::shoulder_over(anim[u].1, anim[f].1, anim_fwd, [0.0, 0.0, 1.0], side);
            let over = crate::flinch::shoulder_over(shoulder, elbow, fwd, up, side);
            if over <= allowed + 1e-3 {
                continue;
            }
            let held = crate::flinch::shoulder_range(shoulder, elbow, fwd, up, side);
            let (Some(from), Some(to)) = (unit(sub(elbow, shoulder)), unit(sub(held, shoulder)))
            else {
                continue;
            };
            let Some(axis) = unit(cross(from, to)) else {
                continue;
            };
            let angle = dot(from, to).clamp(-1.0, 1.0).acos().min(over - allowed);
            if angle < 1e-4 {
                continue;
            }
            let r = m_from_q(qexp(scale(axis, angle)));
            for &b in &[u, f, h] {
                rots[b] = crate::flinch::mul(&r, &rots[b]);
            }
            for &b in &[f, h] {
                joints[b] = add(
                    shoulder,
                    crate::flinch::apply_m(&r, sub(joints[b], shoulder)),
                );
            }
        }
    }

    /// The body as an unarmed actor's (`unarmed`) or an armed one's: an unarmed body keeps its
    /// forearms and hands out of its torso ([`Body::keep_limbs_out`]); an armed one's arms are the
    /// weapon pose's and its aim's.
    pub fn unarmed(&mut self, unarmed: bool) {
        self.limbs_out = unarmed;
    }

    /// The arms' spin bound this frame: an unarmed body's free arms (it neither braces nor
    /// falls: `free`) slower ([`UNARMED_ARM_SPIN`]); else the tuning's.
    pub fn free_arms(&mut self, free: bool) {
        self.arm_spin = if free && self.limbs_out {
            UNARMED_ARM_SPIN
        } else {
            1.0
        };
    }

    /// Bone `i`'s capsule in a pose (`rots` world rotations, `joints` game units, `units` a
    /// metre): its joint, its segment's end and its radius, as the body has them.
    pub fn capsule_at(&self, rots: &[M3], joints: &[V3], i: usize, units: f32) -> (V3, V3, f32) {
        let a = joints[i];
        let b = add(
            a,
            scale(qrot(qfrom_m(&rots[i]), self.segments[i].seg), units),
        );
        (a, b, self.segments[i].radius * units)
    }

    /// Bone `i`'s capsule now: its joint, its end (m).
    fn capsule(&self, i: usize) -> (V3, V3) {
        let a = self.joint(i);
        (a, add(a, qrot(self.state[i].q, self.segments[i].seg)))
    }

    /// How deep bone `l`'s capsule is in bone `t`'s (m; negative: apart), and the way out of it
    /// for `l` (unit; none when the axes meet).
    fn depth(&self, l: usize, t: usize) -> (f32, Option<V3>) {
        let (a, b) = self.capsule(l);
        let (c, d) = self.capsule(t);
        let (pl, pt) = closest_points(a, b, c, d);
        let gap = sub(pl, pt);
        let r = self.segments[l].radius + self.segments[t].radius;
        (r - length(gap), unit(gap))
    }

    /// The shown pose's arms turned out of the torso (Patrick's 10:47 clip on 454ebb2): `rots` the
    /// shown world rotations, `joints` the shown joints (game units, `units` a metre), `anim` the
    /// animated pose. Each arm whose forearm or hand is deeper in a torso capsule than in the
    /// animation (by more than `slack` units) is turned about its shoulder, the least way out;
    /// its joints are moved with it. `capsule` gives a bone's capsule (two ends and a radius, game
    /// units) from a pose's rotations and joints ([`Body::capsule_at`]: the body's own). The hands
    /// in `skip` (an arm a brace or a grip puts on its goal) are left as they are.
    pub fn clear_arms(
        &self,
        rots: &mut [M3],
        joints: &mut [V3],
        anim: &[(M3, V3)],
        capsule: impl Fn(&[M3], &[V3], usize) -> (V3, V3, f32),
        slack: f32,
        skip: &[usize],
    ) {
        let n = self.segments.len();
        if rots.len() != n || joints.len() != n || anim.len() != n {
            return;
        }
        let deep = |c1: (V3, V3, f32), c2: (V3, V3, f32)| {
            let (p, q) = closest_points(c1.0, c1.1, c2.0, c2.1);
            (c1.2 + c2.2 - length(sub(p, q)), p, unit(sub(p, q)))
        };
        let torso: Vec<usize> = (0..n)
            .filter(|&i| matches!(self.segments[i].role, Role::Pelvis | Role::Spine))
            .collect();
        let anim_rots: Vec<M3> = anim.iter().map(|a| a.0).collect();
        let anim_joints: Vec<V3> = anim.iter().map(|a| a.1).collect();
        for h in 0..n {
            if self.segments[h].role != Role::Hand || skip.contains(&h) {
                continue;
            }
            let Some(f) = self.segments[h].parent else {
                continue;
            };
            let Some(u) = self.segments[f].parent else {
                continue;
            };
            for _ in 0..3 {
                // The deepest of the forearm and the hand past what the animation has.
                let mut worst: Option<(f32, V3, V3)> = None;
                for &l in &[f, h] {
                    for &t in &torso {
                        let allowed = deep(
                            capsule(&anim_rots, &anim_joints, l),
                            capsule(&anim_rots, &anim_joints, t),
                        )
                        .0
                        .max(0.0);
                        let (d, at, way) = deep(capsule(rots, joints, l), capsule(rots, joints, t));
                        let excess = d - allowed - slack;
                        let way = way.unwrap_or([0.0, 0.0, 1.0]);
                        if excess > 0.0 && worst.map_or(true, |w| excess > w.0) {
                            worst = Some((excess, at, way));
                        }
                    }
                }
                let Some((excess, at, way)) = worst else {
                    break;
                };
                let shoulder = joints[u];
                let lever = sub(at, shoulder);
                let Some(axis) = unit(cross(lever, way)) else {
                    break;
                };
                let arm = length(sub(lever, scale(axis, dot(lever, axis)))).max(1.0);
                let r = m_from_q(qexp(scale(axis, (excess / arm).min(0.5))));
                for &b in &[u, f, h] {
                    rots[b] = crate::flinch::mul(&r, &rots[b]);
                }
                for &b in &[f, h] {
                    joints[b] = add(
                        shoulder,
                        crate::flinch::apply_m(&r, sub(joints[b], shoulder)),
                    );
                }
            }
        }
    }

    /// The forearms and hands pushed out of the torso (Patrick's 10:47 clip on 454ebb2: arms
    /// through the body): a limb bone deeper in a torso bone's capsule than its pose allowed
    /// ([`Body::outside`]) is moved straight out by the excess; the torso is not moved. A pinned
    /// hand's arm (a planted brace hand, `pins`) is left to its pin.
    fn keep_limbs_out(&mut self, pins: &[(usize, V3)]) {
        for k in 0..self.outside.len() {
            let (l, t, allowed) = self.outside[k];
            let pinned = pins
                .iter()
                .any(|&(h, _)| h == l || self.segments.get(h).and_then(|s| s.parent) == Some(l));
            if pinned {
                continue;
            }
            let (deep, way) = self.depth(l, t);
            let excess = deep - allowed;
            if excess <= 1e-4 {
                continue;
            }
            let way = way.unwrap_or_else(|| {
                unit(sub(self.state[l].com, self.state[t].com)).unwrap_or([0.0, 0.0, 1.0])
            });
            let s = &mut self.state[l];
            s.com = add(s.com, scale(way, excess));
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
        self.hinged_with(rot, TARGET_FLEX)
    }

    /// [`Body::hinged`] with the targets' flex off straight `low` (rad).
    pub fn hinged_with(&self, rot: &[Q], low: f32) -> Option<Vec<Q>> {
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
            // The targets a little off straight (a person's joints seldom lock), so a limb flung
            // straight by its motion is not driven onto its stop.
            if let Some(turn) = hinge_limit_from(u, f, axis, low, flex, lean) {
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
        // The targets' little flex off straight goes with the body's weight: none once it is out
        // (it popped 3.4 degrees in one frame as the body ended: Patrick's 10:47 clip).
        let wanted = self
            .hinged_with(target, TARGET_FLEX * w.clamp(0.0, 1.0))
            .unwrap_or_else(|| target.to_vec());
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

    /// The world rotations `rot` with the hinge of bone `i` (an elbow, a knee) flexed `angle`
    /// (rad) more about its axis (a hurt leg's sag, `crate::condition::Condition::sag`); its
    /// children keep their world rotations (a foot stays level).
    pub fn flex(&self, rot: &mut [Q], i: usize, angle: f32) {
        let (Some(p), Some(axis)) = (
            self.segments.get(i).and_then(|s| s.parent),
            self.hinge.get(i).copied().flatten(),
        ) else {
            return;
        };
        if rot.len() != self.state.len() || !angle.is_finite() || angle == 0.0 {
            return;
        }
        let n = qrot(rot[p], axis);
        rot[i] = qnorm(qmul(qexp(scale(n, angle)), rot[i]));
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
                // A knee more (its foot's pin pulls it straight), an elbow gently (a firm stop on
                // an arm made spin of its own).
                let share = if self.segments[i].role == Role::Calf {
                    KNEE_SHARE
                } else {
                    HINGE_SHARE
                };
                let turn = qexp(scale(qlog(turn), share));
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

    /// How far each bone's joint is from where its parent holds it (m), the most of them, and
    /// that bone: 0 for a body whose joints all hold (`hold_joints`).
    pub fn joint_gap(&self) -> (f32, Option<usize>) {
        let mut most = (0.0f32, None);
        for i in 0..self.state.len() {
            let Some(p) = self.segments[i].parent else {
                continue;
            };
            let at = add(
                self.joint(p),
                qrot(self.state[p].q, self.segments[i].anchor),
            );
            let gap = length(sub(at, self.joint(i)));
            if gap > most.0 {
                most = (gap, Some(i));
            }
        }
        most
    }

    /// The bones with no parent in the body (the pelvis alone in a whole rig: another is a free
    /// piece no joint, motor or limit holds; `physics_parents` gives every bone one).
    pub fn roots(&self) -> Vec<usize> {
        (0..self.segments.len())
            .filter(|&i| self.segments[i].parent.is_none())
            .collect()
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
            self.pushes
                .push((b, scale(dir, p * share * KICK_SHARE), local, -delay));
        }
        for (d, add_) in self.dip.iter_mut().zip(dips) {
            // A dip weakens a motor (up to 0.9); a negative one makes it stronger (an armed body's
            // arms keeping their aim).
            *d = (*d + add_).clamp(-5.0, 0.9);
        }
        self.counter_brace(at, p);
        i
    }

    /// Counter-side bracing (after Kickback's idea): a hit of momentum `p` (N s) at `at` (m)
    /// softens the bones on its side of the body and stiffens those away from it, each by how far
    /// to that side it is (across the pelvis), more for a harder hit (its push over 1 m/s, at most
    /// 2).
    fn counter_brace(&mut self, at: V3, p: f32) {
        let centre = self.state[self.pelvis].com;
        let Some(toward) = unit(sub(at, centre)) else {
            return;
        };
        let k = (p / self.mass.max(1.0)).clamp(0.0, 2.0);
        for b in 0..self.state.len() {
            let Some(r) = unit(sub(self.state[b].com, centre)) else {
                continue;
            };
            let s = dot(r, toward);
            let add_ = if s > 0.0 {
                COUNTER_SOFT * k * s
            } else {
                COUNTER_STIFF * k * s
            };
            self.dip[b] = (self.dip[b] + add_).clamp(-5.0, 0.9);
        }
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
            if self.limbs_out {
                self.keep_limbs_out(&targets.hands);
            }
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
                t.arm_spin_room * self.arm_spin
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
            let most = role.spin() * t.arm_spin_room * self.arm_spin;
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
            // The chest's and the neck's joints (a spine bone on a spine bone, the neck, the head):
            // the waist keeps the limbs' motor, so the torso bends without whirling the arms.
            let strength = match self.segments[i].role {
                Role::Neck | Role::Head => soft_strength(strength, tuning::get().neck_soft),
                Role::Spine
                    if self.segments[i]
                        .parent
                        .is_some_and(|p| self.segments[p].role == Role::Spine) =>
                {
                    soft_strength(strength, tuning::get().torso_soft)
                }
                _ => strength,
            };
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
            // A planted brace hand (`crate::brace`): stiff and critically damped on the arm.
            hand: crate::brace::pin_gains(self.mass * ARM_SHARE),
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
    /// `at` (a foot's or a hand's pin target, m) brought within its limb's straight reach from its
    /// root (the bone's grandparent's joint: hip, knee, ankle; shoulder, elbow, wrist), a hair
    /// short of straight.
    fn within_reach(&self, foot: usize, at: V3) -> V3 {
        let Some(calf) = self.segments.get(foot).and_then(|s| s.parent) else {
            return at;
        };
        let Some(thigh) = self.segments[calf].parent else {
            return at;
        };
        let hip = self.joint(thigh);
        let reach =
            (length(self.segments[thigh].seg) + length(self.segments[calf].seg)) * REACH_SHORT;
        let d = sub(at, hip);
        let l = length(d);
        if l > reach && l.is_finite() {
            add(hip, scale(d, reach / l))
        } else {
            at
        }
    }

    /// Whether hand pin `n` acts: a plain pin always; a contact while its wrist is at or inside
    /// its rest on the surface (the pin would push it out, not pull it in) and near it (within
    /// [`CONTACT_NEAR`]: a hand that has not got there yet is not dragged to it).
    fn hand_presses(&self, targets: &Targets, n: usize) -> bool {
        let Some(&(i, at)) = targets.hands.get(n) else {
            return false;
        };
        let normal = targets.hand_normal.get(n).copied().unwrap_or([0.0; 3]);
        if length(normal) < 0.5 || i >= self.state.len() {
            return true;
        }
        let d = sub(at, self.joint(i));
        dot(d, normal) >= 0.0 && length(d) <= CONTACT_NEAR
    }

    fn pins(&mut self, targets: &Targets, gains: &Gains, h: f32) {
        let mut list: Vec<(usize, V3, f32)> = Vec::new();
        if gains.support.0 > 0.0 {
            list.push((self.pelvis, targets.pelvis, gains.support.0));
        }
        // A foot's pin no further from its hip than the leg reaches straight: a pin past it would
        // pull the knee straight and on, backwards (stepping down a stair).
        list.extend(
            targets
                .feet
                .iter()
                .map(|&(i, at)| (i, self.within_reach(i, at), gains.foot.0)),
        );
        // A hand's pin likewise no further than the arm reaches straight from its shoulder; a
        // contact's only while it presses.
        list.extend(
            targets
                .hands
                .iter()
                .enumerate()
                .filter_map(|(n, &(i, at))| {
                    let share = targets
                        .hand_share
                        .get(n)
                        .copied()
                        .unwrap_or(1.0)
                        .clamp(0.0, 1.0);
                    self.hand_presses(targets, n)
                        .then(|| (i, self.within_reach(i, at), gains.hand.0 * share))
                }),
        );
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
        list.extend(targets.hands.iter().enumerate().filter_map(|(n, &(i, _))| {
            let share = targets
                .hand_share
                .get(n)
                .copied()
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            self.hand_presses(targets, n)
                .then(|| (i, gains.hand.1 * share.sqrt()))
        }));
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
    /// The body carried along with its animation's own motion since the frame before: each bone
    /// from where its animated frame was (`from`: world rotation and joint, m) to where it is now
    /// (`to`), its motion turned with it. The body then works only its offset from the animation:
    /// the actor's AI walking and turning it, a new animation set, never fought or left behind
    /// (Patrick's 14-16-02 clip). The caller leaves out the reaction's own moves of the actor (the
    /// steps' follow of the feet).
    pub fn carry(&mut self, from: &[(Q, V3)], to: &[(Q, V3)]) {
        let n = self.state.len().min(from.len()).min(to.len());
        for i in 0..n {
            let ((q0, p0), (q1, p1)) = (from[i], to[i]);
            let d = qnorm(qmul(q1, qconj(q0)));
            if !(d.iter().all(|x| x.is_finite())
                && p0.iter().chain(p1.iter()).all(|x| x.is_finite()))
            {
                continue;
            }
            let s = &mut self.state[i];
            s.com = add(p1, qrot(d, sub(s.com, p0)));
            s.q = qnorm(qmul(d, s.q));
            s.v = qrot(d, s.v);
            s.w = qrot(d, s.w);
        }
    }

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
    hinge_limit_from(u, f, axis, -HYPEREXTEND, flex, lean)
}

/// [`hinge_limit`] with the range's low end at `low` (rad of flex).
pub fn hinge_limit_from(u: V3, f: V3, axis: V3, low: f32, flex: f32, lean: f32) -> Option<Q> {
    let n = unit(axis)?;
    let (a, l) = hinge_angles(u, f, n);
    let tau = std::f32::consts::TAU;
    let a2 = if a < low {
        let to_low = low - a;
        let to_high = (a + tau) - flex;
        if to_high < to_low {
            flex
        } else {
            low
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

    /// One non-toppling hit's body is handed back from [`REACTION_MOST`] after it, busy or not; a
    /// fall's is not; before then the weight is the reaction's own.
    #[test]
    fn a_reaction_is_handed_back_by_its_time() {
        let t = tuning::get();
        // Busy all along (a brace held): full until its time, then eased out, gone after the
        // hand-back.
        assert_eq!(reaction_weight(0.0, 1.0, false, None), 1.0);
        assert_eq!(reaction_weight(0.0, REACTION_MOST, false, None), 1.0);
        let mid = reaction_weight(0.0, REACTION_MOST + t.blend_back * 0.5, false, None);
        assert!(mid > 0.2 && mid < 0.8, "{mid}");
        assert!(reaction_weight(0.0, REACTION_MOST + t.blend_back + 1e-3, false, None) <= 1e-5);
        // Going over: its own.
        assert_eq!(reaction_weight(0.0, 10.0, true, None), 1.0);
        // Never more than its own weight.
        for s in [0.0, 1.0, 1.5, 2.0, 3.0] {
            assert!(reaction_weight(s, 0.5, false, None) <= weight(s) + 1e-6);
        }
    }

    /// A reaction yields to its animation fast: handed back over [`YIELD_BACK`] from when it
    /// yielded, whatever else holds it; not a fall's. The AI's move or turn past its bounds yields.
    #[test]
    fn a_reaction_yields_to_a_moving_animation() {
        assert_eq!(reaction_weight(0.0, 0.5, false, Some(0.0)), 1.0);
        let mid = reaction_weight(0.0, 0.5, false, Some(YIELD_BACK * 0.5));
        assert!(mid > 0.2 && mid < 0.8, "{mid}");
        assert!(reaction_weight(0.0, 0.5, false, Some(YIELD_BACK + 1e-3)) <= 1e-5);
        assert_eq!(reaction_weight(0.0, 0.5, true, Some(1.0)), 1.0);
        assert!(!yields(YIELD_MOVE * 0.5, YIELD_TURN * 0.5));
        assert!(yields(YIELD_MOVE, 0.0) && yields(0.0, YIELD_TURN));
        assert!(ai_walking(WALKING_SPEED, 0.0) && ai_walking(0.0, -TURNING_SPEED));
        assert!(!ai_walking(WALKING_SPEED * 0.5, TURNING_SPEED * 0.5));
    }

    /// A reaction's offsets are held to its push: a bone's yaw and turn, the pelvis's offset, and
    /// how fast a bone's offset changes (a spin round the waist cannot happen); a free bone is
    /// left alone; a turn past half way round is taken the short way.
    #[test]
    fn a_reactions_offsets_are_held_to_its_push() {
        let anim = vec![Q_ID; 3];
        let (angle, yaw, pelvis_most) = offset_most(0.53);
        assert!((pelvis_most - 6.24).abs() < 1e-3 && angle < 0.7 && yaw < 0.12);
        let mut limit = Limit::default();
        let mut shown = vec![Q_ID; 3];
        let mut p = [0.0; 3];
        limit.hold(&mut shown, &anim, &mut p, 0.53, 1.0 / 60.0, |_| false);
        // A yaw of a full half turn, a bend of a radian, a free bone; the pelvis 20 units off.
        let mut shown = vec![
            qexp([0.0, 0.0, 3.0]),
            qexp([1.0, 0.0, 0.0]),
            qexp([0.0, 0.0, 2.0]),
        ];
        let mut p = [20.0, 0.0, 0.0];
        let held = limit.hold(&mut shown, &anim, &mut p, 0.53, 1.0 / 60.0, |b| b == 2);
        assert_eq!(held, 3);
        let step = PULL_TURN / 60.0 + 1e-4;
        assert!(length(qlog(shown[0])) <= step, "{:?}", qlog(shown[0]));
        assert!(length(qlog(shown[1])) <= step);
        assert!((length(qlog(shown[2])) - 2.0).abs() < 1e-4);
        assert!((length(p) - pelvis_most).abs() < 1e-3);
        // Held there frame after frame: never past the bounds, the yaw never past its own.
        for _ in 0..120 {
            let mut s = vec![qexp([0.0, 0.0, 3.0]), qexp([1.0, 0.0, 0.0]), Q_ID];
            let mut p = [0.0; 3];
            limit.hold(&mut s, &anim, &mut p, 0.53, 1.0 / 60.0, |_| false);
            assert!(yaw_of(s[0]).abs() <= yaw + 1e-4, "{}", yaw_of(s[0]));
            assert!(length(qlog(s[1])) <= angle + 1e-4);
        }
    }

    /// A settling body's ceilings come down with its weight: an offset that holds still is
    /// brought down, never frozen.
    #[test]
    fn a_settling_body_never_freezes() {
        let target = vec![Q_ID; 1];
        let mut settle = Settle::default();
        let mut s = vec![qexp([0.0, 0.4, 0.0])];
        let mut p = [10.0, 0.0, 0.0];
        settle.hold_down(&mut s, &target, &mut p, 1.0, |_| false);
        let mut last = (0.4f32, 10.0f32);
        for k in 0..10 {
            let mut s = vec![qexp([0.0, 0.4, 0.0])];
            let mut p = [10.0, 0.0, 0.0];
            settle.hold_down(&mut s, &target, &mut p, 0.9, |_| false);
            let now = (length(qlog(s[0])), length(p));
            assert!(
                now.0 < last.0 && now.1 < last.1,
                "frame {k}: {now:?} after {last:?}"
            );
            last = now;
        }
    }

    /// The ragdoll takes the body's motion held: the pelvis's speed to the hit's, a flung arm's
    /// motion about the pelvis to the spin bound.
    #[test]
    fn the_handover_holds_a_flung_arm() {
        let s = |com: V3, v: V3| State {
            com,
            q: Q_ID,
            v,
            w: [0.0; 3],
        };
        let state = [
            s([0.0, 0.0, 1.0], [3.0, 0.0, 0.0]),
            s([0.5, 0.0, 1.3], [3.0, 60.0, 0.0]),
            s([0.0, 0.0, 1.4], [3.5, 0.0, 0.0]),
        ];
        let v = handover_velocities(&state, 0, 1.5);
        assert!((length(v[0]) - 1.5).abs() < 1e-4);
        let r = length(sub(state[1].com, state[0].com));
        assert!(
            (length(sub(v[1], v[0])) - HANDOVER_SPIN * r).abs() < 1e-3,
            "{:?}",
            v[1]
        );
        // A bone moving with the pelvis keeps its own small motion.
        assert!((length(sub(v[2], v[0])) - 0.5).abs() < 1e-4);
    }

    /// A body carried with its animation's motion keeps its offset from it: turned and moved
    /// with each bone's frame, its motion turned too.
    #[test]
    fn a_body_rides_on_its_animation() {
        let mut body = Body::from_pose(&figure(), 1.0 / 60.0);
        let before: Vec<(Q, V3)> = body.pose();
        let turn = qexp([0.0, 0.0, 1.0]);
        let moved = [0.5, 0.0, 0.0];
        let after: Vec<(Q, V3)> = before
            .iter()
            .map(|&(q, p)| (qnorm(qmul(turn, q)), add(qrot(turn, p), moved)))
            .collect();
        body.state[0].v = [1.0, 0.0, 0.0];
        body.carry(&before, &after);
        let now = body.pose();
        for (a, n) in after.iter().zip(&now) {
            assert!(length(sub(a.1, n.1)) < 1e-4, "{:?} {:?}", a.1, n.1);
            assert!(length(qlog(qmul(a.0, qconj(n.0)))) < 1e-4);
        }
        assert!(length(sub(body.state[0].v, qrot(turn, [1.0, 0.0, 0.0]))) < 1e-5);
    }

    /// A settling body is never shown gaining on its targets: each bone's offset and the pelvis's
    /// only shrink (a growing one is held where it was, a shrinking one taken); a free bone is left
    /// alone; opened, it holds nothing.
    #[test]
    fn a_settling_body_only_comes_closer() {
        let target = vec![Q_ID; 3];
        let turned = |a: f32| qexp([0.0, 0.0, a]);
        let mut settle = Settle::default();
        let mut shown = vec![turned(0.3), turned(0.2), turned(0.5)];
        let mut pelvis = [3.0, 0.0, 0.0];
        settle.hold(&mut shown, &target, &mut pelvis, |_| false);
        assert!(settle.holding());
        // Growing: held at the frame before's (the spin's sign kept); the free bone grows.
        let mut shown = vec![turned(0.6), turned(-0.4), turned(0.9)];
        let mut pelvis = [0.0, 8.0, 0.0];
        settle.hold(&mut shown, &target, &mut pelvis, |b| b == 2);
        assert!((length(qlog(shown[0])) - 0.3).abs() < 1e-4);
        assert!((length(qlog(shown[1])) - 0.2).abs() < 1e-4);
        assert!(qlog(shown[1])[2] < 0.0);
        assert!((length(qlog(shown[2])) - 0.9).abs() < 1e-4);
        assert!((length(pelvis) - 3.0).abs() < 1e-4 && pelvis[1] > 0.0);
        // Shrinking: taken, and the new least holds.
        let mut shown = vec![turned(0.1), turned(0.1), turned(0.1)];
        let mut pelvis = [1.0, 0.0, 0.0];
        settle.hold(&mut shown, &target, &mut pelvis, |_| false);
        let mut again = vec![turned(0.3), turned(0.3), turned(0.3)];
        let mut p2 = [2.0, 0.0, 0.0];
        settle.hold(&mut again, &target, &mut p2, |_| false);
        assert!((length(qlog(again[0])) - 0.1).abs() < 1e-4);
        assert!((length(p2) - 1.0).abs() < 1e-4);
        settle.open();
        assert!(!settle.holding());
        let mut free = vec![turned(0.8); 3];
        let mut p3 = [9.0, 0.0, 0.0];
        settle.hold(&mut free, &target, &mut p3, |_| false);
        assert!((length(qlog(free[0])) - 0.8).abs() < 1e-4 && length(p3) == 9.0);
    }

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
            hand_share: vec![],
            hand_normal: vec![],
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
        // The same push into the body whatever share of the hit it takes.
        body.hit(
            20.0 / KICK_SHARE,
            [0.0, 1.0, 0.0],
            [0.2, 0.0, 0.95],
            0.4,
            &[0.0; 8],
            &[],
        );
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
        // (A little under 0.5 m/s since the push goes in over 0.12 s: the motors take some.)
        assert!(arm_peak > 0.4, "the struck arm moves: {arm_peak}");
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

    /// A dropped body's last pose eases out over [`FADE_BACK`]: all of it at the drop, none of it
    /// after, never more than a small part of the way in one 60 Hz frame.
    #[test]
    fn a_dropped_body_fades_back() {
        let anim = [
            m_from_q(qexp([0.0, 0.0, 0.4])),
            m_from_q(qexp([0.3, 0.0, 0.0])),
        ];
        let shown = [
            m_from_q(qexp([0.0, 0.0, 1.9])),
            m_from_q(qexp([-0.5, 0.2, 0.0])),
        ];
        let mut fade = Fade::new(Fade::offsets(&shown, &anim), [12.0, 0.0, -4.0]);
        let angle = |a: &M3, b: &M3| {
            2.0 * length(qlog(qmul(qfrom_m(a), qconj(qfrom_m(b))))).min(std::f32::consts::PI)
        };
        let mut now = anim;
        let mut pelvis = [0.0; 3];
        fade.on(&mut now, &mut pelvis);
        for b in 0..2 {
            assert!(angle(&now[b], &shown[b]) < 1e-3, "bone {b} at the drop");
        }
        assert!(length(sub(pelvis, [12.0, 0.0, -4.0])) < 1e-4);
        let mut last = now;
        let mut frames = 0;
        while fade.advance(1.0 / 60.0) {
            let mut now = anim;
            let mut p = [0.0; 3];
            fade.on(&mut now, &mut p);
            for b in 0..2 {
                // The quintic's steepest is 1.875 of the mean speed: 18 frames.
                let most = angle(&shown[b], &anim[b]) * 1.9 / 18.0;
                let step = angle(&now[b], &last[b]);
                assert!(step < most, "bone {b} jumped {step} rad in one frame");
            }
            last = now;
            frames += 1;
        }
        assert!((17..=19).contains(&frames), "{frames} frames");
        let mut now = anim;
        let mut p = [0.0; 3];
        fade.on(&mut now, &mut p);
        for b in 0..2 {
            assert!(angle(&now[b], &anim[b]) < 1e-3, "bone {b} after the fade");
        }
        assert!(length(p) < 1e-4);
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

    /// A shown ankle under its floor is lifted onto it by the leg's IK (the thigh and calf
    /// turned, the foot's own kept), across where it was; one on or over it is left.
    #[test]
    fn a_sunk_foot_is_lifted_onto_its_floor() {
        use crate::flinch::{apply_m, IDENTITY};
        let (hip, knee, ankle) = ([0.0, 0.0, 90.0], [0.0, 3.0, 50.0], [0.0, 0.0, 6.0]);
        let mut shown = vec![IDENTITY; 3];
        let at = [hip, knee, ankle];
        let fwd = [0.0, 1.0, 0.0];
        assert_eq!(lift_feet(&mut shown, &at, &[(0, 1, 2)], &[10.0], fwd), 1);
        let knee2 = add(hip, apply_m(&shown[0], sub(knee, hip)));
        let ankle2 = add(knee2, apply_m(&shown[1], sub(ankle, knee)));
        assert!((ankle2[2] - 10.0).abs() < 0.2, "{ankle2:?}");
        assert!(ankle2[0].abs() < 0.2 && ankle2[1].abs() < 0.2, "{ankle2:?}");
        assert_eq!(shown[2], IDENTITY);
        // On its floor already: nothing turned.
        let mut same = vec![IDENTITY; 3];
        assert_eq!(lift_feet(&mut same, &at, &[(0, 1, 2)], &[5.0], fwd), 0);
        assert_eq!(same, vec![IDENTITY; 3]);
    }

    /// A leg-only jump right after a hit is re-based; anything else, or later, yields. The kept
    /// offset re-based keeps its world rotation.
    #[test]
    fn a_leg_jump_at_the_hit_is_re_based() {
        assert!(rebases(0.03, true, &[Role::Calf]));
        assert!(rebases(0.03, true, &[]));
        assert!(rebases(0.2, true, &[Role::Calf, Role::Foot, Role::Thigh]));
        assert!(!rebases(0.25, true, &[Role::Calf]));
        assert!(!rebases(0.03, true, &[Role::Calf, Role::Spine]));
        assert!(!rebases(0.03, false, &[]));
        let still = (crate::flinch::IDENTITY, [0.0; 3]);
        let turned = |a: f32| (m_from_q(qexp([a, 0.0, 0.0])), [0.0; 3]);
        assert!(leg_jumped(&still, &turned(0.3)));
        assert!(!leg_jumped(&still, &turned(0.05)));
        let mut limit = Limit::default();
        let anim = vec![qexp([0.1, 0.0, 0.0])];
        let mut shown = vec![qexp([0.3, 0.0, 0.2])];
        let mut p = [0.0; 3];
        limit.hold(&mut shown, &anim, &mut p, 2.0, 1.0 / 60.0, |_| false);
        let world = qmul(limit.before[0], anim[0]);
        let to = qexp([0.0, 0.4, 0.0]);
        limit.rebase(0, anim[0], to);
        let again = qmul(limit.before[0], to);
        // The same rotation (either sign of the quaternion).
        assert!(qmul(again, qconj(world))[0].abs() > 1.0 - 1e-5);
    }

    /// The lift grows and shrinks toward what is wanted no faster than `LIFT_RISE`.
    #[test]
    fn the_lift_eases_in_and_out() {
        let r = LIFT_RISE / 60.0;
        assert!((lift_amount(0.0, 5.0, 1.0 / 60.0) - r).abs() < 1e-5);
        assert!((lift_amount(0.5, 0.6, 1.0 / 60.0) - 0.6).abs() < 1e-5);
        assert!((lift_amount(3.0, 0.0, 1.0 / 60.0) - (3.0 - r)).abs() < 1e-5);
        assert_eq!(lift_amount(0.2, -4.0, 1.0 / 60.0), 0.0);
    }

    /// A bone drawn more than `MOST_DRAWN_OFF` from its animation (or not finite) is found, the
    /// farthest; the bones not checked are not.
    #[test]
    fn a_bone_drawn_far_off_is_found() {
        let anim = [[0.0; 3], [0.0, 0.0, 50.0], [10.0, 0.0, 90.0]];
        let near = [[1.0, 0.0, 0.0], [0.0, 3.0, 50.0], [10.0, 0.0, 95.0]];
        assert_eq!(too_far(&near, &anim, |_| true), None);
        let far = [[1.0, 0.0, 0.0], [0.0, 40.0, 50.0], [10.0, 0.0, 1100.0]];
        assert_eq!(too_far(&far, &anim, |_| true).map(|f| f.0), Some(2));
        assert_eq!(too_far(&far, &anim, |i| i != 2).map(|f| f.0), Some(1));
        let nan = [[f32::NAN, 0.0, 0.0], [0.0, 0.0, 50.0], [10.0, 0.0, 90.0]];
        assert_eq!(too_far(&nan, &anim, |_| true).map(|f| f.0), Some(0));
    }
}
