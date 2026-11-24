//! New Physics' controller for one actor: its body ([`crate::np_body`], FNV's ragdoll) driven
//! toward the animation by its muscles ([`crate::np_drive`]), held up at its root and balanced on
//! its feet ([`crate::np_balance`]: the capture point, the steps); a hit is an impulse on the struck
//! body and a softening of its chain; each frame answers the pose to draw (every body's node's
//! world). The host and the ragdoll lab run this same code.
//!
//! The root's support carries the body's weight (the legs' strength, up and down) and, across, only
//! as much as the ankles and hips can while the capture point is inside the feet
//! ([`ROOT_SUPPORT`]'s across cap times the balance's hold): past it the body drifts and steps.

use crate::active::{qconj, qfrom_m, qmul, qnorm, Q};
use crate::flinch::{add, dot, scale, sub, M3, V3};
#[allow(unused_imports)]
use crate::fm::Fm;
use crate::np_balance::{Balance, Phase};
use crate::np_body::{Drive, Model, Plane, State, StepReport, Support, Tracking, UNITS};
use crate::np_down::{Bodies, Label, Labeler, Reading};
use crate::np_drive::Muscles;

/// The root's support: up and down a spring (N/m), damper and cap (N); across the same at most
/// `max_force_h` (scaled by the balance's hold); its turn (N·m/rad, cap N·m). A hit softens it by
/// its share at the root. `body`, `x`, `v` and `q` are set each frame.
pub const ROOT_SUPPORT: Support = Support {
    body: 0,
    x: [0.0; 3],
    v: [0.0; 3],
    q: [1.0, 0.0, 0.0, 0.0],
    kp: 30000.0,
    kd: 2500.0,
    max_force: 2000.0,
    kp_h: 30000.0,
    kd_h: 2500.0,
    max_force_h: 900.0,
    kp_turn: 8000.0,
    kd_turn: 120.0,
    max_torque: 600.0,
};

/// A swinging foot's pull (its body toward the step's arc): spring, damper, cap.
pub const FOOT_PULL: Support = Support {
    body: 0,
    x: [0.0; 3],
    v: [0.0; 3],
    q: [1.0, 0.0, 0.0, 0.0],
    kp: 6000.0,
    kd: 150.0,
    max_force: 1200.0,
    kp_h: 6000.0,
    kd_h: 150.0,
    max_force_h: 1200.0,
    kp_turn: 300.0,
    kd_turn: 20.0,
    max_torque: 150.0,
};

/// A planted foot's hold where it landed (friction's reach across; pressed onto the floor).
pub const PLANT_PULL: Support = Support {
    body: 0,
    x: [0.0; 3],
    v: [0.0; 3],
    q: [1.0, 0.0, 0.0, 0.0],
    kp: 4000.0,
    kd: 100.0,
    max_force: 350.0,
    kp_h: 8000.0,
    kd_h: 200.0,
    max_force_h: 700.0,
    kp_turn: 200.0,
    kd_turn: 10.0,
    max_torque: 80.0,
};

/// Moving on its path: the legs' springs and dampers times these.
pub const MOVING_LEGS: (f32, f32) = (4.0, 2.0);

/// Falling: the muscles' share (the ragdoll's until the game takes it, M4).
pub const FALLING_MUSCLES: f32 = 0.25;

/// The animation's velocity is measured across this much of its recent frames (s), once it has
/// at least [`ANIM_V_LEAST`] of them.
pub const ANIM_V_WINDOW: f32 = 0.12;
pub const ANIM_V_LEAST: f32 = 0.06;

/// `pose` (body-node worlds, game units) moved by `by` (game units).
fn shift_by(pose: &[(M3, V3)], by: V3) -> Vec<(M3, V3)> {
    pose.iter().map(|(r, x)| (*r, add(*x, by))).collect()
}

/// A rise's pelvis must gain this (m) to count as progress.
pub const RISE_GAINED: f32 = 0.01;
/// The centre's fall (m/s) past which a rising body is still coming down, within the rise's first
/// [`RISE_LANDING`] seconds (a rise wished as it lands).
pub const RISE_COMING_DOWN: f32 = 0.2;
pub const RISE_LANDING: f32 = 1.0;
/// A rise's progress (m of the pelvis's height) that starts its stall's clock again.
pub const RISE_PROGRESS: f32 = 0.01;
/// The centre's climb (m/s) at which a rise past [`RISE_MOST`] is still going up.
pub const RISE_GOING_UP: f32 = 0.3;
/// The pelvis's share of its standing height at which a rise past [`RISE_MOST`] is nearly up
/// (it slowed through 0.88 m with the torso still coming up and was given up).
pub const RISE_NEARLY_UP: f32 = 0.8;
/// Rising, a joint's goal lies at most this far from where the joint is (rad): the muscles' effort
/// a frame is bounded (goals given whole threw the body off the floor).
pub const RISE_REACH: f32 = 0.3;
/// Rising, the spine's and legs' springs' share of standing's (times the effort); the legs hold
/// none of their weight (a hold on legs and torso together floated the body off the floor).
pub const RISE_GAIN: f32 = 0.6;
/// Rising, the torso leans toward the feet by this much a metre the centre of mass lies behind
/// them (rad/m), at most this (rad).
pub const RISE_LEAN: (f32, f32) = (2.5, 1.0);
/// Rising, each foot is drawn to its goal on the floor: spring (N/m), damper (N·s/m), cap (N,
/// times the effort).
pub const RISE_FEET: (f32, f32, f32) = (2000.0, 120.0, 250.0);
/// Rising, the arms' push turning the torso up: spring (N·m/rad), damper (N·m·s/rad), cap (N·m,
/// times the effort), and how near a surface the torso's centre must be for a hand to reach it
/// (m); only while the torso tilts past this (rad).
pub const RISE_ARMS: (f32, f32, f32, f32) = (3000.0, 150.0, 160.0, 0.6);
pub const RISE_ARMS_TILT: f32 = 0.5;
/// A rise's hand plant: out from the shoulder and ahead toward the rise (m), and its pull
/// (spring, damper, most force N).
pub const HAND_OUT: f32 = 0.12;
pub const HAND_AHEAD: f32 = 0.08;
pub const HAND_PLANT: (f32, f32, f32) = (3000.0, 150.0, 400.0);
/// Until it touches, the hand is guided to its spot with at most this (N): the arm's muscles put
/// it there (the plant's whole pull yanked the arm, turning the pelvis 20 degrees in a frame).
pub const HAND_REACH: f32 = 60.0;
/// A hand's spot is kept this far off a wall's face past the hand's radius (m); a wall whose
/// normal rises past [`HAND_WALL_FLOOR`] is ground, and one the spot lies more than
/// [`HAND_WALL_BEHIND`] (m) behind is another room's.
pub const HAND_WALL: f32 = 0.05;
pub const HAND_WALL_FLOOR: f32 = 0.64;
pub const HAND_WALL_BEHIND: f32 = 0.3;
/// How long the limits stay soft after a fall (s).
pub const LIMITS_SETTLE: f32 = 0.3;
/// Standing again, the balance's joint targets and the pelvis's turn ease from the pose the body
/// stood up in to the animation's over this long (s): switched at once, the legs and spine driven
/// to the animation's stance pushed a still body to 0.3 to 0.4 m/s in its first frames.
pub const STAND_BLEND: f32 = 0.8;
/// Standing again, the actor taken to the body at most this fast (m/s) for this long (s).
pub const STAND_TAKE: (f32, f32) = (0.05, 1.0);
/// A rise with nothing to push from (or too weak) this long gives up (s).
pub const RISE_NO_PUSH: f32 = 0.5;
/// A rise lasts at most this long (s, from the AI's wish to stand, whatever the frames read):
/// then it gives up (the AI waits, or after its tries the body goes to the game's ragdoll).
/// Patrick's 23:41 clip rose for 7 and 17 s, sliding. Counted from the wish, 2 and 3 s cut
/// short the flat floor's rise (`np_downed_on_fnvs_skeleton_stands_again`: standing again
/// within 8 s); 4 s does not.
pub const RISE_MOST: f32 = 4.0;
/// Standing again: the body's centre slower than this (m/s) against the ground it is carried on
/// ([`Np::ground_v`]).
pub const STAND_STILL: f32 = 0.05;
/// A rise past [`RISE_MOST`] with the body upright is held this long more (s) to come still
/// before the balance takes it whatever its speed.
pub const STILL_MOST: f32 = 2.0;
/// Standing again, the centre of mass at least this far inside the soles (m, negative inside).
pub const STAND_INSIDE: f32 = -0.02;
/// Rising and upright, the feet off the animation's stance (m: [`Np::stance_apart`]) past
/// [`STANCE_KEPT`]: the foot carrying less goes to its place, done once the feet are within
/// [`STANCE_THERE`] (m) of the stance. Standing again only within [`STANCE_KEPT`] (the balance's own stance step comes at
/// `np_balance::STANCE_OFF`).
pub const STANCE_KEPT: f32 = 0.06;
pub const STANCE_THERE: f32 = 0.01;
/// Standing again, the torso against the pelvis at most this far from the animation's (degrees).
pub const STAND_POSE: f32 = 10.0;
/// Rising and upright, the torso's flat sway's damper (N·s/m) and its cap (N).
pub const TORSO_STILL: (f32, f32) = (2500.0, 1000.0);
/// The torso's lean (degrees from upright) up to which [`TORSO_STILL`] damps it in full; less
/// past it, none at [`STILL_TILT_OUT`].
pub const TORSO_STILL_FULL: f32 = 18.0;
/// Rising past this share of standing height with the centre further than [`RISE_OVER_FIRST`]
/// (m) outside its feet, the lift goes at [`RISE_SLOW`] of its pace.
pub const RISE_SLOW_FROM: f32 = 0.6;
pub const RISE_OVER_FIRST: f32 = 0.03;
pub const RISE_SLOW: f32 = 0.3;
/// How far (m) an animated foot may lie in a wall before its leg is bent to keep it out.
pub const WALL_FOOT_IN: f32 = 0.005;
/// While the feet lie further than this from the hips (m, flat) the rise goes toward them.
pub const RISE_FEET_OUT: f32 = 0.15;
/// Rising from the floor, each foot's goal lies this share of its leg in front of its hip (a hip
/// on the floor; under it as the hip comes up a leg high).
pub const RISE_FOOT_OUT: f32 = 0.3;
/// How far (units) a rising foot is held into its ground, so that it carries from the start.
const RISE_FOOT_PRESS: f32 = 0.5;
/// The torso's front pointing up past this (its share of the vertical) is the chest facing the
/// sky; otherwise, with the torso tilted past [`ARMS_UNTIL_TILT`] degrees from upright, the arms
/// keep pushing while the legs do.
pub const CHEST_UP: f32 = 0.5;
pub const ARMS_UNTIL_TILT: f32 = 60.0;
/// The legs push once the centre of mass is within this of the feet (m): their pull brings the
/// hips forward over the feet as they lift.
pub const RISE_OVER_FEET: f32 = 0.35;
/// A rise that begins with the chest facing down (the front's up under minus this) rises from the
/// front: first onto the knees (the pelvis to [`KNEEL_HEIGHT`] of standing height over the knees,
/// each foot [`KNEEL_FOOT`] (m) behind its hip, the torso up), then a foot forward,
/// [`LUNGE_FOOT`] (m) in front of its hip and lifted [`LUNGE_LIFT`] (m) until it is within
/// [`LUNGE_THERE`] (m) of its place; each stage at most [`FRONT_STAGE_MOST`] (s). Rising off the
/// front with the rise's own goals (the torso turned up and leant toward the feet, the feet drawn
/// in front of the hips) it sat back on its heels and rolled onto its seat: a flip.
pub const FRONT_START: f32 = 0.5;
pub const KNEEL_HEIGHT: f32 = 0.5;
pub const KNEEL_FOOT: f32 = 0.35;
pub const LUNGE_FOOT: f32 = 0.3;
pub const LUNGE_LIFT: f32 = 0.08;
pub const LUNGE_THERE: f32 = 0.05;
pub const FRONT_STAGE_MOST: f32 = 2.0;
/// The legs' push brings the pelvis over its place at this rate (1/s) of the way left, at most
/// this fast (m/s), the support's spring at this share and its damper at this many times.
pub const RISE_FLAT: (f32, f32, f32, f32) = (3.0, 0.3, 0.5, 2.0);
/// The legs' push lifts the pelvis at most this fast (m/s), slowing as it nears standing height
/// (its pace per metre still to go, 1/s).
pub const RISE_PACE: (f32, f32) = (0.8, 3.0);
/// The legs' push held through a lapse this long (s).
pub const LIFT_LAPSE: f32 = 0.2;

/// Up enough for the rise to bring the body still over its feet (its feet held, its lean
/// dropped): standing (`classify`), the pelvis at [`STILL_UP`] of standing height and the torso
/// within [`STILL_TILT`] of upright. Earlier, holding the feet stalled the rise. Once there
/// (`was`), it stays there until the pelvis drops under [`STILL_UP_OUT`] or the torso tilts past
/// [`STILL_TILT_OUT`]: on the line itself it flickered frame to frame, the feet and the lean
/// switching with it, and the sway it set off kept the body from coming still (1.04 s upright
/// before still on flat ground).
fn up_for_still(r: &crate::np_down::Reading, was: bool) -> bool {
    let (up, tilt) = if was {
        (STILL_UP_OUT, STILL_TILT_OUT)
    } else {
        (STILL_UP, STILL_TILT)
    };
    // Latched, the classifier's Standing is not asked again: its feet's contact came and went
    // frame to frame on a body standing still (the still stage switched off every other frame and
    // a body 0.98 of standing height, 5 to 9 degrees from upright, never stood again).
    (was || crate::np_down::classify(r, false) == Label::Standing)
        && r.pelvis_height >= up * r.stand_height
        && r.tilt <= tilt.to_radians()
}
/// [`up_for_still`]'s lines: in at these, out at the `_OUT` ones.
pub const STILL_UP: f32 = 0.85;
pub const STILL_TILT: f32 = 25.0;
pub const STILL_UP_OUT: f32 = 0.78;
pub const STILL_TILT_OUT: f32 = 40.0;

/// `now` turned toward `goal` by at most `most` (rad); relative rotations.
fn turn_toward(now: Q, goal: Q, most: f32) -> Q {
    let d = crate::active::qlog(qmul(goal, qconj(now)));
    let a = crate::flinch::length(d);
    if a <= most || a < 1e-6 {
        return goal;
    }
    qnorm(qmul(crate::active::qexp(scale(d, most / a)), now))
}

/// Read standing and still this long (s), a downed body is the balance's again.
pub const STAND_HOLD: f32 = 0.25;

/// The rate (1/s) at which each body's turn off its animation's decays (a hit softens it with the
/// joint's spring).
pub const TRACKING: f32 = 90.0;

/// The share of a joint's softening the tracking of its child body takes.
pub const TRACKING_SOFT: f32 = 0.5;

/// How far the animation may move the root in a frame before the body is set on it (game
/// units): a teleport, a load.
pub const JUMP: f32 = 70.0;

/// One actor's New Physics.
#[derive(Clone, Debug)]
pub struct Np {
    pub model: Model,
    pub states: Vec<State>,
    pub muscles: Muscles,
    /// Seconds since it began, and since the last hit.
    pub t: f32,
    /// The last frame's seconds.
    last_dt: f32,
    pub since_hit: Option<f32>,
    /// The last frame's step.
    pub report: StepReport,
    /// The animated pose's bodies last frame (for the support and a jump).
    anim_states: Vec<State>,
    /// The root support's turn damper: critical for the whole body's inertia about the root.
    root_kd_turn: f32,
    /// The balance: the capture point, the steps, the body's move against its animation.
    pub balance: Balance,
    /// The animation's centre of mass last frame (unshifted, m) for its velocity, and that
    /// velocity (m/s; set by [`Np::set_anim_velocity`] for the first frame).
    anim_com: Option<V3>,
    /// The animation's centre of mass over the last [`ANIM_V_WINDOW`] (the clock's time, m,
    /// local): its velocity is measured across them, frames where the pose did not advance left
    /// out.
    anim_samples: std::collections::VecDeque<(f32, V3)>,
    /// The game's position of the local frame the body is simulated in (game units; the actor's
    /// first root, flat): far from the world's origin an f32's last digits are millimetres, and
    /// a substep's velocity from them tenths of a metre a second.
    pub origin: V3,
    anim_v: V3,
    /// The walls the body may rest on (m; the floor is the frame's).
    pub walls: Vec<Plane>,
    /// Each body's own walls (world, metres): what the world has beside it (its owner's casts).
    pub body_walls: Vec<Vec<Plane>>,
    /// Each of [`Np::body_walls`]' tops (world, metres; infinite: none known).
    pub body_wall_tops: Vec<Vec<f32>>,
    /// Each body's own floor (world, metres: the ground under it, a slope's; empty: the frame's
    /// flat floor).
    pub floors: Vec<Plane>,
    /// Each body's floor under each of its ends (world, metres; empty: [`Np::floors`]).
    pub end_floors: Vec<[Plane; 2]>,
    /// The edges of steps and ledges near the body (world, metres).
    pub edges: Vec<crate::np_body::Edge>,
    /// The arm that holds a weapon (0 left, 1 right; none unarmed): it keeps it, and its aim
    /// while the AI fires from the ground; the free arm pushes.
    pub weapon_side: Option<usize>,
    /// The AI fires from the ground: the weapon arm aims, the free arm braces.
    pub firing: bool,
    /// About to be handed to the game's ragdoll: the body is brought inside the skeleton's own
    /// limits (`np_body::Tracking::data_limits`); `report.limit_most` says how far out it still is.
    pub to_limits: bool,
    /// Each joint's allowance out of its limits while they are soft
    /// (`np_body::Tracking::allow`), and the last frame's limits (soft, the data's, without a
    /// reach): the allowance is measured afresh when any changes.
    limit_allow: Vec<f32>,
    soft_was: (bool, bool, bool),
    /// The last frame ran inside the skeleton's own limits ([`Np::to_limits`]): its
    /// `report.limit_most` is how far out of them the body still was.
    pub on_data_limits: bool,
    /// The animation's jumps so far and the last's size (game units).
    pub jumps: u32,
    pub jump_last: f32,
    /// The ground's velocity under the body (m/s): what the game itself moved the actor by this
    /// frame (none once its movement is held); standing again is measured against it.
    pub ground_v: V3,
    /// Keep a fallen body (M6, downed but active) rather than answer [`Phase::Falling`] for the
    /// game's ragdoll to take it.
    pub keep_down: bool,
    /// Downed: what holds it and how it reads.
    pub down: Option<Down>,
    /// The AI's wish to stand (the controller works back toward balance while it holds).
    pub rise: bool,
    /// The strength left (wounds, a crippled leg, fatigue): 1 whole, 0 none.
    pub effort: f32,
    /// The muscles' strength given at the start (the legs' push scales by it).
    pub strength: f32,
    pub bodies: Bodies,
    /// The joints' targets last frame (the limits widened by them too, `np_body::Tracking`).
    last_targets: Vec<Q>,
    /// Seconds since it last stood up from the floor (long ago: never).
    stood_up: f32,
    /// At the last stand: each joint's relative rotation and the pelvis's rotation, the pose the
    /// balance's targets ease out of over [`STAND_BLEND`].
    stand_from: Option<(Vec<Q>, Q)>,
}

/// A downed body's figures: how long down, the label, the last reading, the rise's progress.
#[derive(Clone, Debug)]
pub struct Down {
    pub since: f32,
    pub labeler: Labeler,
    pub reading: Reading,
    /// A rise's time so far and the pelvis's best height in it (m over the floor).
    pub rising: f32,
    pub rise_best: f32,
    /// The rise's body has stopped coming down once (until then its gains are measured from
    /// its lowest and no stall is counted).
    pub rise_landed: bool,
    /// The pelvis's height when the stall's clock last started again (m): it starts again only
    /// for [`RISE_PROGRESS`] more.
    pub rise_mark: f32,
    /// Seconds since the rise's pelvis last got higher.
    pub rise_stall: f32,
    /// Whether the last rise gave up (too weak, or nothing to push from).
    pub rise_failed: bool,
    /// The legs' push now (N) and whether it lifts; whether the arms push the torso up.
    pub push: f32,
    pub lifting: bool,
    pub lifting_was: bool,
    pub arms: bool,
    /// Seconds the body has read standing.
    pub stood: f32,
    /// Seconds a rise has had nothing to push from (or too little strength).
    pub no_push: f32,
    /// The label changes so far (time, label) for the log.
    pub changes: Vec<(f32, Label)>,
    /// Each hand's plant on the ground in a rise (game units, local): where it was put down,
    /// held there until the legs push.
    pub hand_plants: [Option<V3>; 2],
    /// Each planted hand has touched the ground: from then the ground alone holds it up (it only
    /// pushes), the plant only keeps it from sliding.
    pub hand_landed: [bool; 2],
    /// The legs have taken this rise over: the hands stay let go until it ends (a frame's lapse
    /// in the legs' push planted them again mid-rise).
    pub arms_off: bool,
    /// How long the legs' push has lapsed (s).
    pub lift_lapse: f32,
    /// Upright past [`RISE_MOST`]: held to come still, not given up.
    pub still_wait: bool,
    /// Up for the still stage ([`up_for_still`]) at the last frame.
    pub still_up: bool,
    /// A rise from the front (it began with its chest facing down, [`FRONT_START`]): 1 onto the
    /// knees, 2 a foot forward (the lunge, `front_side`'s), 0 the rise's own push; `front_done`
    /// once past them (not begun again in this rise).
    pub front_stage: u8,
    pub front_side: usize,
    pub front_done: bool,
    pub front_for: f32,
    /// Upright, the foot going to its place in the animation's stance ([`STANCE_KEPT`]).
    pub stance_fixing: Option<usize>,
}

/// Each joint's relative rotation (`parent⁻¹ child`) in a pose of body-node worlds.
pub fn relative_of(model: &Model, worlds: &[(M3, V3)]) -> Vec<Q> {
    model
        .joints
        .iter()
        .map(|j| {
            let a = qfrom_m(&crate::pose::ortho(&worlds[j.child].0));
            let b = qfrom_m(&crate::pose::ortho(&worlds[j.parent].0));
            qnorm(qmul(qconj(b), a))
        })
        .collect()
}

impl Np {
    /// The body set on the animated pose `anim` (each body's node's world, game units), still;
    /// muscles at `strength`.
    pub fn new(model: Model, anim: &[(M3, V3)], strength: f32) -> Np {
        let origin = anim
            .get(model.root)
            .map_or([0.0; 3], |r| [r.1[0].round(), r.1[1].round(), 0.0]);
        let anim = shift_by(anim, scale(origin, -1.0));
        let states = model.states_from_nodes(&anim);
        let muscles = Muscles::new(&model, strength);
        let bodies = Bodies::new(&model);
        let root_kd_turn = ROOT_SUPPORT
            .kd_turn
            .max(2.0 * (ROOT_SUPPORT.kp_turn * model.inertia_about_root()).sqrt());
        let balance = Balance::new(&model);
        Np {
            balance,
            anim_com: None,
            anim_samples: std::collections::VecDeque::new(),
            origin,
            anim_v: [0.0; 3],
            walls: Vec::new(),
            floors: Vec::new(),
            end_floors: Vec::new(),
            edges: Vec::new(),
            body_walls: Vec::new(),
            body_wall_tops: Vec::new(),
            weapon_side: None,
            firing: false,
            to_limits: false,
            limit_allow: Vec::new(),
            soft_was: (false, false, true),
            on_data_limits: false,
            jumps: 0,
            jump_last: 0.0,
            ground_v: [0.0; 3],
            keep_down: false,
            down: None,
            rise: false,
            effort: 1.0,
            strength,
            bodies,
            last_targets: Vec::new(),
            stood_up: f32::INFINITY,
            stand_from: None,
            last_dt: 0.0,
            root_kd_turn,
            anim_states: states.clone(),
            states,
            muscles,
            model,
            t: 0.0,
            since_hit: None,
            report: StepReport::default(),
        }
    }

    /// A hit: `impulse` (N·s, world) on body `body` at `at` (world, game units).
    pub fn hit(&mut self, body: usize, at: V3, impulse: V3) {
        if body >= self.states.len() || !impulse.iter().all(|x| x.is_finite()) {
            return;
        }
        let at = sub(at, self.origin);
        self.model
            .impulse(&mut self.states, body, scale(at, 1.0 / UNITS), impulse);
        let size = crate::flinch::length(impulse);
        self.muscles.hit(&self.model, body, size);
        self.since_hit = Some(0.0);
        // Each hit's catching steps are its own (steps were counted across hits while the
        // animation moved: 13 by Patrick's 23:41 clip, and a 0.69 m/s hit fell "steps").
        self.balance.steps = 0;
    }

    /// One frame of `dt`: the body driven toward `anim` (each body's node's world, game units),
    /// moved by the balance's shift, the floor at `floor` (game units); answers each body's node's
    /// world to draw.
    pub fn frame(&mut self, anim: &[(M3, V3)], floor: f32, dt: f32) -> Vec<(M3, V3)> {
        // Simulated about `origin`: in, and back out.
        let local = shift_by(anim, scale(self.origin, -1.0));
        let to_m = 1.0 / UNITS;
        let walls = std::mem::take(&mut self.walls);
        self.walls = walls
            .iter()
            .map(|w| Plane {
                p: sub(w.p, scale(self.origin, to_m)),
                n: w.n,
            })
            .collect();
        let body_wall_tops = std::mem::take(&mut self.body_wall_tops);
        self.body_wall_tops = body_wall_tops
            .iter()
            .map(|ts| ts.iter().map(|t| t - self.origin[2] * to_m).collect())
            .collect();
        let body_walls = std::mem::take(&mut self.body_walls);
        self.body_walls = body_walls
            .iter()
            .map(|ws| {
                ws.iter()
                    .map(|w| Plane {
                        p: sub(w.p, scale(self.origin, to_m)),
                        n: w.n,
                    })
                    .collect()
            })
            .collect();
        let floors = std::mem::take(&mut self.floors);
        self.floors = floors
            .iter()
            .map(|w| Plane {
                p: sub(w.p, scale(self.origin, to_m)),
                n: w.n,
            })
            .collect();
        let end_floors = std::mem::take(&mut self.end_floors);
        self.end_floors = end_floors
            .iter()
            .map(|ends| {
                ends.map(|w| Plane {
                    p: sub(w.p, scale(self.origin, to_m)),
                    n: w.n,
                })
            })
            .collect();
        let edges = std::mem::take(&mut self.edges);
        self.edges = edges
            .iter()
            .map(|ed| crate::np_body::Edge {
                p: sub(ed.p, scale(self.origin, to_m)),
                ..*ed
            })
            .collect();
        let out = self.frame_local(&local, floor, dt);
        self.walls = walls;
        self.floors = floors;
        self.end_floors = end_floors;
        self.edges = edges;
        self.body_walls = body_walls;
        self.body_wall_tops = body_wall_tops;
        shift_by(&out, self.origin)
    }

    fn frame_local(&mut self, anim: &[(M3, V3)], floor: f32, dt: f32) -> Vec<(M3, V3)> {
        self.on_data_limits = self.to_limits;
        let unshifted = self.model.states_from_nodes(anim);
        let (com_now, _) = self.model.centre_of_mass(&unshifted);
        let jumped = self
            .anim_com
            .is_some_and(|was| crate::flinch::length(sub(com_now, was)) * UNITS > JUMP);
        if jumped {
            self.anim_samples.clear();
            self.jumps += 1;
            self.jump_last = self
                .anim_com
                .map_or(0.0, |was| crate::flinch::length(sub(com_now, was)) * UNITS);
        }
        // The animation's velocity across the last [`ANIM_V_WINDOW`] of frames whose pose
        // advanced (at least [`ANIM_V_LEAST`] of them; until then the seed): a frame's own
        // difference over its time was the noise of a fraction of a unit at 250 frames a second.
        let now_t = self.t + dt;
        let advanced = self.anim_samples.back().map_or(true, |&(_, c)| {
            crate::flinch::length(sub(com_now, c)) > 1e-6
        });
        if dt > 0.0 && advanced {
            self.anim_samples.push_back((now_t, com_now));
            while self.anim_samples.len() > 2
                && self
                    .anim_samples
                    .get(1)
                    .is_some_and(|s| now_t - s.0 >= ANIM_V_WINDOW)
            {
                self.anim_samples.pop_front();
            }
            if let (Some(&(t0, c0)), Some(&(t1, c1))) =
                (self.anim_samples.front(), self.anim_samples.back())
            {
                if t1 - t0 >= ANIM_V_LEAST {
                    self.anim_v = scale(sub(c1, c0), 1.0 / (t1 - t0));
                }
            }
        } else if dt > 0.0
            && self
                .anim_samples
                .back()
                .map_or(true, |&(t, _)| now_t - t >= ANIM_V_WINDOW)
        {
            // Not moved for the window: still (a seed of the walk it was taken in, or the last
            // measure of a reference the game then held, was kept for good: the balance read a
            // downed body's rise as a walk, its rise stalled at the pelvis 0.65 m).
            self.anim_v = [0.0; 3];
        }
        let anim_v = self.anim_v;
        self.anim_com = Some(com_now);
        let shift = self.balance.shift;
        let anim_states: Vec<State> = unshifted
            .iter()
            .map(|s| State {
                x: add(s.x, shift),
                ..*s
            })
            .collect();
        let r = self.model.root;
        // The animation's turn of each body this frame (rad/s).
        let anim_w: Vec<V3> = if dt > 0.0 && !jumped {
            anim_states
                .iter()
                .zip(&self.anim_states)
                .map(|(a, b)| scale(crate::active::qlog(qmul(a.q, qconj(b.q))), 1.0 / dt))
                .collect()
        } else {
            vec![[0.0; 3]; anim_states.len()]
        };
        self.anim_states = anim_states;
        // A jump of the animation (a teleport, a load) puts the body back on it, never while it
        // is down (the downed body snapped upright on a jump read from torn locals: the tester's
        // 08:13 clip).
        if jumped && self.down.is_none() {
            self.states = self.anim_states.clone();
            self.balance = Balance::new(&self.model);
        }
        self.t += dt;
        self.last_dt = dt;
        if let Some(s) = self.since_hit.as_mut() {
            *s += dt;
        }
        self.muscles.advance(dt);
        if self.down.is_some() {
            return self.down_frame(anim, floor, dt);
        }
        // Each foot's own ground plane (a slope's).
        for side in 0..2 {
            self.balance.foot_floor[side] = self.floors.get(self.balance.feet[side]).copied();
        }
        self.balance.edges = self.edges.clone();
        self.balance.since_hit = self.since_hit.unwrap_or(f32::INFINITY);
        let ask = self.balance.update(
            &self.model,
            &self.states,
            &self.anim_states,
            anim_v,
            floor / UNITS,
            dt,
        );
        // Engaged, the legs' targets are the animation's bent onto the footing (`leg_ik`): the
        // hips lowered as the feet spread, each ankle where its foot stands, lands or swings (the
        // animation's straight stance leg held the hips up, and a stepped foot hung in the air).
        let shifted: Vec<(M3, V3)> = anim
            .iter()
            .map(|(r, x)| (*r, add(*x, scale(shift, UNITS))))
            .collect();
        if ask.falling && self.keep_down {
            // Past any step: the body is kept, falling, and then held on whatever it lands on.
            self.down = Some(Down {
                since: 0.0,
                labeler: Labeler::new(Label::Standing),
                reading: Reading::default(),
                rising: 0.0,
                rise_best: 0.0,
                rise_landed: false,
                rise_mark: 0.0,
                rise_stall: 0.0,
                rise_failed: false,
                push: 0.0,
                lifting: false,
                lifting_was: false,
                arms: false,
                stood: 0.0,
                no_push: 0.0,
                changes: Vec::new(),
                hand_plants: [None, None],
                hand_landed: [false, false],
                arms_off: false,
                lift_lapse: 0.0,
                still_wait: false,
                still_up: false,
                stance_fixing: None,
                front_stage: 0,
                front_side: 0,
                front_done: false,
                front_for: 0.0,
            });
            return self.down_frame(anim, floor, dt);
        }
        let posed = self.legs_on_footing(&shifted, &ask);
        let targets = relative_of(&self.model, posed.as_deref().unwrap_or(anim));
        // Just stood up: the targets eased out of the pose it stood up in ([`STAND_BLEND`]).
        let blend = self
            .stand_from
            .as_ref()
            .filter(|_| self.stood_up < STAND_BLEND)
            .map(|(from, q)| {
                (
                    from.clone(),
                    *q,
                    crate::active::ease(self.stood_up / STAND_BLEND),
                )
            });
        let targets: Vec<Q> = match &blend {
            Some((from, _, k)) if from.len() == targets.len() => targets
                .iter()
                .zip(from)
                .map(|(t, f)| crate::active::qslerp(*f, *t, *k))
                .collect(),
            _ => targets,
        };
        let root_q = match &blend {
            Some((_, q, k)) => crate::active::qslerp(*q, self.anim_states[r].q, *k),
            None => self.anim_states[r].q,
        };
        let mut gains = self.muscles.gains();
        for side in 0..2 {
            for &j in &self.balance.legs[side] {
                gains[j].0 *= ask.leg[side];
            }
        }
        // Walking or running on its path, the legs keep to the gait (at the table's standing
        // values the hips trailed a walk by up to 20 degrees, the feet by 30 units).
        if self.balance.moving && ask.foot.is_none() {
            for side in 0..2 {
                for &j in &self.balance.legs[side] {
                    gains[j].0 *= MOVING_LEGS.0;
                    gains[j].1 *= MOVING_LEGS.1;
                }
            }
        }
        if ask.falling {
            for g in &mut gains {
                g.0 *= FALLING_MUSCLES;
            }
        }
        let drives: Vec<Drive> = targets
            .iter()
            .zip(gains)
            .map(|(&target, (kp, kd))| Drive {
                target,
                kp,
                kd,
                hold: if ask.falling { FALLING_MUSCLES } else { 1.0 },
            })
            .collect();
        let mut pulls = Vec::new();
        if !ask.falling {
            let soft = 1.0 - 0.5 * self.muscles.root_softening();
            pulls.push(Support {
                body: r,
                x: sub(self.anim_states[r].x, [0.0, 0.0, ask.lower]),
                v: anim_v,
                q: root_q,
                kp: ROOT_SUPPORT.kp * soft,
                kd: ROOT_SUPPORT.kd * soft,
                kp_h: ROOT_SUPPORT.kp_h * soft,
                kd_h: ROOT_SUPPORT.kd_h * soft,
                max_force_h: ROOT_SUPPORT.max_force_h * ask.hold_across,
                kp_turn: ROOT_SUPPORT.kp_turn * soft,
                kd_turn: self.root_kd_turn * soft,
                ..ROOT_SUPPORT
            });
        }
        for (body, x, q) in ask.plants.iter().flatten().copied() {
            if !ask.falling {
                pulls.push(Support {
                    body,
                    x,
                    q,
                    ..PLANT_PULL
                });
            }
        }
        if let Some((body, x, q, v)) = ask.foot {
            pulls.push(Support {
                body,
                x,
                v,
                q,
                ..FOOT_PULL
            });
        }
        // Tracking softened with the joint each body hangs from (the root with the support).
        let soft = self.muscles.softening();
        let base = if ask.falling { 0.0 } else { TRACKING };
        let mut rate =
            vec![base * (1.0 - TRACKING_SOFT * self.muscles.root_softening()); self.states.len()];
        for (j, def) in self.model.joints.iter().enumerate() {
            rate[def.child] = base * (1.0 - TRACKING_SOFT * soft[j]);
        }
        // A swinging leg follows its foot, not its animation's turn.
        if let Some(st) = self.balance.step {
            for &j in &self.balance.legs[st.side] {
                rate[self.model.joints[j].child] *= 0.2;
            }
        }
        // Moving on its path, the feet go where the walk puts them (no grip); a step, a plant or
        // standing grips.
        let mut slip = Vec::new();
        if self.balance.moving && ask.foot.is_none() {
            for side in 0..2 {
                if ask.plants[side].is_none() {
                    slip.push(self.balance.feet[side]);
                }
            }
        }
        let grip = self.balance.feet.to_vec();
        let tracking = Tracking {
            w: anim_w,
            rate,
            slip,
            grip,
            // Standing, the limits as M2 had them (the last targets are kept for a fall).
            before: {
                self.last_targets = targets.clone();
                Vec::new()
            },
            widen_by: Vec::new(),
            // Just stood up from the floor: the downed limits eased out, as they were eased in.
            reach: if self.stood_up < LIMITS_SETTLE {
                self.bodies.reach.clone()
            } else {
                Vec::new()
            },
            // Soft on past the reach's end (its widening dropped under the pose is eased in as
            // well: dropped and hard at once, the pose was snapped in, 5 body weights on the
            // floor).
            soft_limits: self.stood_up < 2.0 * LIMITS_SETTLE || self.to_limits,
            allow: Vec::new(),
            floors: self.floors.clone(),
            end_floors: self.end_floors.clone(),
            edges: self.edges.clone(),
            data_limits: self.to_limits,
            body_walls: self.body_walls.clone(),
            body_wall_tops: self.body_wall_tops.clone(),
        };
        let tracking = self.with_allowance(tracking);
        self.stood_up += dt;
        self.report = self.model.step_world(
            &mut self.states,
            &drives,
            &pulls,
            Some(&tracking),
            floor / UNITS,
            &self.walls,
            dt,
        );
        self.limit_allow = std::mem::take(&mut self.report.allow);
        self.model.node_worlds(&self.states)
    }

    /// `tracking` given each joint's allowance out of its limits ([`Tracking::allow`]): the
    /// last frame's while the limits stay soft, none (measured afresh) as they go soft.
    fn with_allowance(&mut self, mut tracking: Tracking) -> Tracking {
        let now = (
            tracking.soft_limits,
            tracking.data_limits,
            tracking.reach.is_empty(),
        );
        if tracking.soft_limits && self.soft_was == now {
            tracking.allow = std::mem::take(&mut self.limit_allow);
        }
        self.soft_was = now;
        tracking
    }

    /// The legs' push now (N): [`crate::np_down::RISE_FORCE`] times strength and effort.
    pub fn push(&self) -> f32 {
        crate::np_down::RISE_FORCE * self.strength.clamp(0.5, 1.5) * self.effort.clamp(0.0, 1.0)
    }

    /// One downed frame (M6): the body held on whatever touches it, falling with its muscles let
    /// go until something does; with [`Np::rise`] the controller works back toward balance: the
    /// feet drawn under the hips, the torso over the feet, then the legs' push. Nothing poses the
    /// body: the joints aim at the animation (or, rising, at goals: a foot under its hip, the
    /// torso over the feet) with springs that hold rather than stand.
    fn down_frame(&mut self, anim: &[(M3, V3)], floor: f32, dt: f32) -> Vec<(M3, V3)> {
        use crate::np_down::{classify, read, reads, turn_between, DOWN_ARMS, DOWN_GAINS};
        let f = floor / UNITS;
        let push = self.push();
        let b = self.bodies.clone();
        let r = self.model.root;
        let mut reading = read(&self.model, &b, &self.states, &self.report, f);
        let was_lifting = self.down.as_ref().is_some_and(|d| d.lifting);
        if was_lifting {
            reading.pushed = (reading.weight - reading.total).max(0.0);
        }
        let rising_wish = self.rise;
        let rd = reads(&reading, None, push);
        let mut down = self.down.take().unwrap_or_else(|| Down {
            since: 0.0,
            labeler: Labeler::new(Label::Lying),
            reading,
            rising: 0.0,
            rise_best: 0.0,
            rise_landed: false,
            rise_mark: 0.0,
            rise_stall: 0.0,
            rise_failed: false,
            push,
            lifting: false,
            lifting_was: false,
            arms: false,
            stood: 0.0,
            no_push: 0.0,
            changes: Vec::new(),
            hand_plants: [None, None],
            hand_landed: [false, false],
            arms_off: false,
            lift_lapse: 0.0,
            still_wait: false,
            still_up: false,
            stance_fixing: None,
            front_stage: 0,
            front_side: 0,
            front_done: false,
            front_for: 0.0,
        });
        down.since += dt;
        down.reading = reading;
        down.push = push;
        // Nothing under it to push from, or too weak: the wish waits (a moment's lift of the
        // legs as they fold is not a failure), and gives up once it has lasted.
        if rising_wish && !rd.can_push_up {
            down.no_push += dt;
        } else {
            down.no_push = 0.0;
        }
        let rising = rising_wish && down.no_push < RISE_NO_PUSH;
        if let Some(l) = down.labeler.update(classify(&reading, rising), dt) {
            down.changes.push((down.since, l));
        }
        let airborne = reading.total < 0.15 * reading.weight;
        // The joints' targets: the animation's; rising, the legs onto goals under the hips.
        let mut targets = relative_of(&self.model, anim);
        let anim_rel = targets.clone();
        let mut gains = self.muscles.gains();
        let mut holds = vec![0.0f32; gains.len()];
        let k = self.effort.clamp(0.0, 1.0);
        for (j, def) in self.model.joints.iter().enumerate() {
            let name = self.model.bodies[def.child]
                .node
                .trim_start_matches("Bip01 ");
            let name = name.trim_start_matches(['L', 'R']).trim_start();
            let (share, hold) = DOWN_GAINS
                .iter()
                .find(|g| {
                    if g.0 == "Spine" {
                        name.starts_with("Spine")
                    } else {
                        name == g.0
                    }
                })
                .map_or((DOWN_ARMS, 0.0), |g| (g.1, g.2));
            if airborne {
                gains[j].0 *= FALLING_MUSCLES;
                holds[j] = FALLING_MUSCLES;
            } else {
                gains[j].0 *= share * (0.3 + 0.7 * k);
                holds[j] = hold * k;
            }
        }
        let mut pulls = Vec::new();
        down.lifting_was = down.lifting;
        down.lifting = false;
        down.arms = false;
        // A rise's clock runs while the wish holds, whatever the frame reads (a body tumbling on
        // a slope read airborne now and then, and each such frame began the rise again: the
        // clip rose for 7 and 17 s).
        if rising_wish {
            down.rising += dt;
        } else {
            down.front_stage = 0;
            down.front_done = false;
        }
        if rising && !airborne {
            // Up already: no more height to gain, coming still is the rise's end (not a stall).
            if !down.rise_landed
                && down.rising < RISE_LANDING
                && reading.com_v[2] < -RISE_COMING_DOWN
            {
                // Still coming down (a rise wished as it lands): its gains are measured from the
                // lowest it comes to, and no stall is counted meanwhile (measured from the
                // height it fell through, a landing rise gave up 1.5 s after it touched down).
                down.rise_best = down.rise_best.min(reading.pelvis_height);
                down.rise_mark = down.rise_best;
                down.rise_stall = 0.0;
            } else if {
                down.rise_landed |= down.rising >= RISE_LANDING;
                reading.pelvis_height > down.rise_best + RISE_GAINED
                    || classify(&reading, false) == Label::Standing
            } {
                down.rise_best = down.rise_best.max(reading.pelvis_height);
                // The clock starts again for real progress only (a body bobbing face down at
                // 0.3 to 0.45 m set a new best by a centimetre now and then and never stalled).
                if reading.pelvis_height > down.rise_mark + RISE_PROGRESS
                    || classify(&reading, false) == Label::Standing
                {
                    down.rise_mark = reading.pelvis_height;
                    down.rise_stall = 0.0;
                } else {
                    down.rise_stall += dt;
                }
            } else {
                down.rise_stall += dt;
            }
            let worlds = self.model.node_worlds(&self.states);
            let feet_mid = scale(add(self.states[b.feet[0]].x, self.states[b.feet[1]].x), 0.5);
            // A rise from the front ([`FRONT_START`]): onto the knees, a foot forward, then the
            // rise's own push.
            let front_up = {
                let fw = b.forward_of(&self.states, b.torso);
                fw[2] / crate::flinch::length(fw).max(1e-6)
            };
            // Judged once, as the rise begins from lying (a rise from the back pitches its front
            // down as it comes over its feet: judged on, it was pulled back down to kneel).
            // Armed only (one arm pushes, the weapon kept): both arms push the chest up and back
            // over the feet, which a single arm could not (on knees and one hand its feet never
            // took its weight); unarmed the rise's own push stands it in about 4 s.
            if down.front_stage == 0
                && !down.front_done
                && down.rising <= dt
                && down.labeler.label == Label::Lying
                && front_up < -FRONT_START
                && self.weapon_side.is_some()
            {
                down.front_stage = 1;
                down.front_for = 0.0;
                // The foot of the free arm's side forward (the weapon arm keeps its aim).
                down.front_side = self.weapon_side.map_or(0, |w| 1 - w);
            }
            down.front_for += dt;
            let kneel_z = KNEEL_HEIGHT * b.stand_height;
            if down.front_stage == 1
                && (reading.pelvis_height >= 0.85 * kneel_z && reading.tilt < 35f32.to_radians()
                    || down.front_for > FRONT_STAGE_MOST)
            {
                down.front_stage = 2;
                down.front_for = 0.0;
            } else if down.front_stage == 2 && down.front_for > FRONT_STAGE_MOST {
                down.front_stage = 0;
                down.front_done = true;
            }
            let front_stage = down.front_stage;
            // The body's way from its chest to its pelvis (flat): behind it, where the feet go
            // kneeling.
            let back_dir = crate::flinch::unit({
                let d = sub(self.states[r].x, self.states[b.torso].x);
                [d[0], d[1], 0.0]
            })
            .unwrap_or(reading.facing);
            // The way it rises: toward its feet while they lie out from the hips (on its back
            // the torso's facing points at the sky), the torso's facing once they are under it.
            let to_feet = sub(feet_mid, self.states[r].x);
            let to_feet = [to_feet[0], to_feet[1], 0.0];
            let rise_dir = if crate::flinch::length(to_feet) > RISE_FEET_OUT {
                crate::flinch::unit(to_feet).unwrap_or(reading.facing)
            } else {
                reading.facing
            };
            let toward = sub(feet_mid, reading.com);
            let flat = [toward[0], toward[1], 0.0];
            // The torso over the feet: up, leaning toward them by how far behind it is; upright,
            // straight up (leaning on, the hips chased the feet and the feet the hips: a creep of
            // 0.2 to 0.3 m/s that never came still, Patrick's 22:19 clip).
            let upright = up_for_still(&reading, down.still_up);
            down.still_up = upright;
            let lean = if upright || front_stage != 0 {
                0.0
            } else {
                (crate::flinch::length(flat) * RISE_LEAN.0).min(RISE_LEAN.1)
            };
            let dir = crate::flinch::unit(flat).unwrap_or(rise_dir);
            let want_up = add(scale([0.0, 0.0, 1.0], lean.dcos()), scale(dir, lean.dsin()));
            let c = turn_between(b.up_of(&self.states, b.torso), want_up);
            // The arms' push off the floor (or a wall) behind and beside it, while the torso is
            // low enough for a hand to reach (its centre within an arm of a surface): the torso
            // turned toward the lean wanted, capped by strength (the legs' push is the root's
            // support's: the same abstraction for the arms).
            let torso_x = self.states[b.torso].x;
            let reach = self
                .walls
                .iter()
                .fold(torso_x[2] - f, |m, w| m.min(dot(sub(torso_x, w.p), w.n)));
            if reach < RISE_ARMS.3 && reading.tilt > RISE_ARMS_TILT {
                let q = self.states[b.torso].q;
                pulls.push(Support {
                    body: b.torso,
                    x: torso_x,
                    v: [0.0; 3],
                    q: qnorm(qmul(crate::active::qexp(c), q)),
                    kp: 0.0,
                    kd: 0.0,
                    max_force: 0.0,
                    kp_h: 0.0,
                    kd_h: 0.0,
                    max_force_h: 0.0,
                    kp_turn: RISE_ARMS.0,
                    kd_turn: RISE_ARMS.1,
                    max_torque: RISE_ARMS.2 * k,
                });
                down.arms = true;
            }
            let spines: Vec<usize> = self
                .model
                .joints
                .iter()
                .enumerate()
                .filter(|(_, j)| b.spines.contains(&j.child))
                .map(|(i, _)| i)
                .collect();
            let share = 1.0 / spines.len().max(1) as f32;
            for &j in &spines {
                let def = &self.model.joints[j];
                let qc = self.states[def.child].q;
                let qp = self.states[def.parent].q;
                let goal = qmul(crate::active::qexp(scale(c, share)), qc);
                let now = qnorm(qmul(qconj(qp), qc));
                // Upright, the spine goes to the animation's own (its twist and its curve too:
                // turned only to bring the torso's up straight, Spine2 stood 18 to 41 degrees off
                // the animation's, and straightened after the hand-over it swung the centre at
                // 0.45 m/s).
                let goal = if upright {
                    targets[j]
                } else {
                    qnorm(qmul(qconj(qp), goal))
                };
                targets[j] = turn_toward(now, goal, RISE_REACH);
                // Upright, standing's own spring (at the rise's share it took 2.5 s to bring the
                // spine round to the animation's, the torso rocking meanwhile).
                gains[j].0 = self.muscles.base[j].0 * if upright { 1.0 } else { RISE_GAIN } * k;
                holds[j] = if upright { 1.0 } else { 0.5 } * k;
            }
            // Upright with its feet off the animation's stance ([`STANCE_KEPT`]): the foot
            // carrying less (further from the centre) goes to its place in the stance beside the
            // other before it stands (a stance step taken after standing moved it 3.8 units on,
            // Patrick's drift bound 2).
            let stance = self.stance_apart(anim);
            if !upright {
                down.stance_fixing = None;
            } else if down.stance_fixing.is_none() && stance.0 > STANCE_KEPT {
                // The foot that goes is chosen once (chosen each frame, it changed as the
                // centre moved and the step never ended).
                down.stance_fixing = Some(stance.1);
            } else if stance.0 <= STANCE_THERE {
                down.stance_fixing = None;
            }
            let stance_fix = down
                .stance_fixing
                .map(|side| (side, self.stance_place(anim, side)));
            // Each foot drawn onto the floor in front of its hip, as near as the leg folds (half
            // its length out from a hip near the floor, under it once the hip is a leg high).
            let mut posed = worlds.clone();
            for side in 0..2 {
                let (thigh, calf, foot) = (b.thighs[side], b.calves[side], b.feet[side]);
                let hip = worlds[thigh].1;
                let leg = self.balance.leg_length[side] * UNITS;
                // The foot's node as high as its capsule set on the ground as it lies now, held a
                // little into it ([`RISE_FOOT_PRESS`]) (the
                // animation's own foot is mid-stride while the actor runs: held at that height
                // the feet hovered over the floor, never carried and the legs never pushed).
                let foot_h = {
                    let q = self.states[foot].q;
                    let def = &self.model.bodies[foot];
                    let low = def
                        .ends
                        .iter()
                        .map(|e| crate::active::qrot(q, *e)[2])
                        .fold(f32::INFINITY, f32::min);
                    let press = if upright { 0.0 } else { RISE_FOOT_PRESS };
                    let set = (def.radius - low) * UNITS - press;
                    let animated =
                        (anim[foot].1[2] - anim[r].1[2] + b.stand_height * UNITS).max(5.0);
                    if upright {
                        set
                    } else {
                        set.min(animated)
                    }
                };
                // This foot's own ground under its hip (a slope's), game units.
                let ground = self.ground_at(foot, hip).map_or(floor, |z| z * UNITS);
                let up = ((hip[2] - ground - foot_h) / leg).clamp(0.0, 1.0);
                // Under the hip once up (5% of a leg in front kept the hips chasing the feet: a
                // creep of 0.3 m/s, the rise ending moving and tripping, Patrick's 22:19 clip).
                let out = leg * RISE_FOOT_OUT * (1.0 - up);
                let ahead = scale(rise_dir, out);
                // Upright, each foot stays where it is, set down on its own ground (followed
                // under the hips, the feet and the hips chased each other).
                let at = if front_stage == 1 {
                    // Kneeling: the foot behind its hip, the knee under it.
                    [
                        hip[0] + back_dir[0] * KNEEL_FOOT * UNITS,
                        hip[1] + back_dir[1] * KNEEL_FOOT * UNITS,
                        0.0,
                    ]
                } else if front_stage == 2 {
                    if side == down.front_side {
                        // The lunge: in front of its hip.
                        [
                            hip[0] - back_dir[0] * LUNGE_FOOT * UNITS,
                            hip[1] - back_dir[1] * LUNGE_FOOT * UNITS,
                            0.0,
                        ]
                    } else {
                        let ft = worlds[foot].1;
                        [ft[0], ft[1], 0.0]
                    }
                } else if let Some((_, to)) = stance_fix.filter(|f| f.0 == side) {
                    to
                } else if upright {
                    let ft = worlds[foot].1;
                    [ft[0], ft[1], 0.0]
                } else {
                    [hip[0] + ahead[0], hip[1] + ahead[1], 0.0]
                };
                let ground = self.ground_at(foot, at).map_or(floor, |z| z * UNITS);
                // The foot going to its place in the stance slides there, kept in the support
                // (lifted, it tipped a body still moving on a 20 degree slope to 39 degrees).
                let lift = 0.0;
                // The lunging foot lifted while away from its place (dragged, it caught).
                let lift = if front_stage == 2 && side == down.front_side {
                    let ft = worlds[foot].1;
                    let away = crate::flinch::length([at[0] - ft[0], at[1] - ft[1], 0.0]);
                    if away > LUNGE_THERE * UNITS {
                        LUNGE_LIFT * UNITS
                    } else {
                        // There: the rise's own push from here on.
                        down.front_stage = 0;
                        down.front_done = true;
                        0.0
                    }
                } else {
                    lift
                };
                let goal = [at[0], at[1], ground + foot_h + lift];
                // Kneeling or lunging, the knees bend toward the head.
                let knee_way = if front_stage != 0 {
                    scale(back_dir, -1.0)
                } else {
                    rise_dir
                };
                let Some((r1, m, _)) = crate::flinch::leg_ik(
                    worlds[thigh].1,
                    worlds[calf].1,
                    worlds[foot].1,
                    hip,
                    goal,
                    knee_way,
                ) else {
                    continue;
                };
                let mr1 = crate::flinch::mul(&m, &r1);
                posed[thigh].0 = crate::flinch::mul(&r1, &worlds[thigh].0);
                posed[calf].0 = crate::flinch::mul(&mr1, &worlds[calf].0);
                posed[foot].0 = anim[foot].0;
                // The foot drawn in along the floor (the leg's muscles against the floor's
                // grip: a heel dragged, not a leg swung up); once the legs push the feet stand
                // where they are; upright, each pressed onto its own ground where it is.
                if down.lifting_was && !upright && front_stage == 0 {
                    continue;
                }
                let q = self.states[foot].q;
                let com = self.model.bodies[foot].com;
                let x = add(scale(goal, 1.0 / UNITS), crate::active::qrot(q, com));
                pulls.push(Support {
                    body: foot,
                    x,
                    v: [0.0; 3],
                    q,
                    kp: RISE_FEET.0,
                    kd: RISE_FEET.1,
                    max_force: RISE_FEET.2 * k,
                    kp_h: RISE_FEET.0,
                    kd_h: RISE_FEET.1,
                    max_force_h: RISE_FEET.2 * k,
                    kp_turn: 0.0,
                    kd_turn: 0.0,
                    max_torque: 0.0,
                });
            }
            // The arms' push (the free arm; both unarmed; the weapon arm keeps its aim): each
            // hand put down on the ground beside its shoulder, toward the rise, and held where it
            // lands (no slide, no bounce); the arm driven straight under a shoulder raised by
            // half the arm, so it pushes the torso up from lying, seated or propped; let go once
            // the legs push. The arms were held at the animation's sides before, the torso
            // turned by an abstract support alone (Patrick's 10:08 and 10:10 clips).
            // ... not while its chest still faces the ground (from its front the legs pushed at
            // once and the arms let go with the chest down: the hands carried nothing, Patrick's
            // design has the hands pushing the chest up first).
            // (Not facing the sky, with the torso still past `ARMS_UNTIL_TILT` from upright.)
            let chest = b.forward_of(&self.states, b.torso);
            let chest_up = chest[2] / crate::flinch::length(chest).max(1e-6) > CHEST_UP;
            let chest_low = !chest_up && reading.tilt > ARMS_UNTIL_TILT.to_radians();
            down.arms_off |= down.lifting_was && !chest_low;
            let arms = if down.arms_off {
                Vec::new()
            } else {
                // Kneeling or lunging from the front, the free arm only (its shoulder up out of
                // its reach there is no ledge to brace on: the weapon's hand went down on flat
                // ground, 155 frames).
                let arms = self.pushing_arms(&worlds, floor);
                match self.weapon_side {
                    Some(w) if front_stage != 0 => arms.into_iter().filter(|&s| s != w).collect(),
                    _ => arms,
                }
            };
            self.plant_hands(
                &mut down, &arms, &worlds, floor, rise_dir, &mut posed, &mut pulls,
            );
            let legs = relative_of(&self.model, &posed);
            self.drive_planted_arms(&down, &legs, &mut targets, &mut gains, &mut holds);
            let now_rel = self.model.relative(&self.states);
            for side in 0..2 {
                for &j in &self.balance.legs[side] {
                    targets[j] = turn_toward(now_rel[j], legs[j], RISE_REACH);
                    gains[j].0 = self.muscles.base[j].0 * RISE_GAIN * k;
                    holds[j] = 0.0;
                }
            }
            // Upright, the arms, neck and head at standing's own strength toward the animation's
            // (held at the downed share until the hand-over, they swung into its pose after it
            // and moved the centre at 0.07 m/s).
            if upright && down.arms_off {
                for (j, def) in self.model.joints.iter().enumerate() {
                    let leg = self.balance.legs.iter().any(|l| l.contains(&j));
                    if leg || b.spines.contains(&def.child) {
                        continue;
                    }
                    gains[j].0 = self.muscles.base[j].0 * k;
                    holds[j] = k;
                }
            }
            // Over a foot that is down (one knee still on the floor is a kneel to rise from):
            // the legs' push lifts the pelvis toward standing.
            let feet_down = b
                .feet
                .iter()
                .any(|&ft| self.report.floor_load.get(ft).copied().unwrap_or(0.0) > 1.0);
            // A lapse of a few frames (a foot's load or the hips over it read off for a frame)
            // keeps the push: each lapse let the feet be pulled again and the hands planted.
            let over = feet_down && reading.over_feet < RISE_OVER_FEET;
            if over {
                down.lift_lapse = 0.0;
            } else if down.lifting_was {
                down.lift_lapse += dt;
            }
            if front_stage != 0 {
                // Onto the knees and over them while a foot comes forward: the pelvis held at
                // kneeling height where it is (flat motion damped toward the ground's).
                let ground_here = self
                    .ground_at(r, scale(self.states[r].x, UNITS))
                    .unwrap_or(f);
                let z = ground_here + kneel_z;
                let pace =
                    ((z - self.states[r].x[2]) * RISE_PACE.1).clamp(-RISE_PACE.0, RISE_PACE.0);
                pulls.push(Support {
                    body: r,
                    x: self.states[r].x,
                    v: [self.ground_v[0], self.ground_v[1], pace],
                    // The hips brought upright (a tall kneel, the torso over them).
                    q: self.anim_states[r].q,
                    kp: ROOT_SUPPORT.kp,
                    kd: ROOT_SUPPORT.kd,
                    max_force: push,
                    kp_h: 0.0,
                    kd_h: ROOT_SUPPORT.kd_h * 0.5,
                    max_force_h: 400.0 * k,
                    kp_turn: ROOT_SUPPORT.kp_turn * 0.5,
                    kd_turn: self.root_kd_turn * 0.5,
                    max_torque: 300.0 * k,
                });
            } else if over || upright || (down.lifting_was && down.lift_lapse < LIFT_LAPSE) {
                down.lifting = true;
                // Over the feet's own ground (a slope's).
                let feet_ground = b
                    .feet
                    .iter()
                    .filter_map(|&ft| self.ground_at(ft, scale(self.states[ft].x, UNITS)))
                    .fold(None, |m: Option<f32>, z| Some(m.map_or(z, |m| m.max(z))));
                let stand_z = feet_ground.unwrap_or(f) + b.stand_height;
                let up_now = ((self.states[r].x[2] - feet_ground.unwrap_or(f)) / b.stand_height)
                    .clamp(0.0, 1.0);
                // Damped against the rise's own pace, not against rising (damped toward still, the
                // capped spring's push and an uncapped damper held the pelvis to 0.3 to 0.4 m/s:
                // a rise of 2 s and more).
                // Past [`RISE_SLOW_FROM`] of standing height with its feet down and the centre
                // still outside them, the lift at [`RISE_SLOW`] of its pace (the lift outran the
                // carry: the centre 0.10 m outside the feet at 80% of standing height, the lab's
                // M6 stand; capped there instead, a host rise beside the slab went otherwise).
                let slow = if !upright
                    && reading.over_feet > RISE_OVER_FIRST
                    && reading.over_feet < 1.0
                    && self.states[r].x[2]
                        > feet_ground.unwrap_or(f) + RISE_SLOW_FROM * b.stand_height
                {
                    RISE_SLOW
                } else {
                    1.0
                };
                let pace =
                    ((stand_z - self.states[r].x[2]) * RISE_PACE.1).clamp(0.0, RISE_PACE.0) * slow;
                // Its flat motion damped toward the ground's (the ground the body stands on may
                // carry it), harder as it comes up: the rise ends still over its feet.
                // Upright, the pelvis set so the centre of mass lies over the feet (the pelvis
                // over them, a torso still leaning left the centre beside them on a slope).
                let over_at = if upright {
                    // Where the animation's stance puts the pelvis against its feet (the balance
                    // then takes it with its animation on the feet as they stand, the pelvis
                    // already where its root support holds it: placed by the centre of mass
                    // instead, it moved 2 to 3 units after standing, the reference with it).
                    let fb = self.balance.feet;
                    let a = &self.anim_states;
                    let anim_mid = scale(add(a[fb[0]].x, a[fb[1]].x), 0.5);
                    let off = sub(a[r].x, anim_mid);
                    [feet_mid[0] + off[0], feet_mid[1] + off[1]]
                } else {
                    [feet_mid[0], feet_mid[1]]
                };
                // Brought over the feet at a pace that falls as it nears its place (led there by
                // the spring alone, it overshot and came upright still moving 0.4 to 0.5 m/s,
                // 0.7 s to come still; and the lift outran it: the centre 0.07 to 0.09 m outside
                // the feet at 80% of standing height, the lab's M6 stand): the support damps
                // toward that pace, its spring a share.
                let to = [
                    over_at[0] - self.states[r].x[0],
                    over_at[1] - self.states[r].x[1],
                ];
                let d = (to[0] * to[0] + to[1] * to[1]).sqrt();
                let along = (d * RISE_FLAT.0).min(RISE_FLAT.1);
                let approach = if d > 1e-4 {
                    [to[0] / d * along, to[1] / d * along]
                } else {
                    [0.0; 2]
                };
                pulls.push(Support {
                    body: r,
                    x: [over_at[0], over_at[1], stand_z],
                    v: [
                        self.ground_v[0] + approach[0],
                        self.ground_v[1] + approach[1],
                        pace,
                    ],
                    q: self.anim_states[r].q,
                    kp: ROOT_SUPPORT.kp,
                    kd: ROOT_SUPPORT.kd,
                    max_force: push,
                    // Held over the feet harder as it comes up: near upright the sway is the
                    // stand's to stop (it swayed 0.36 to 0.56 m/s upright and never stood:
                    // Patrick's 10:10 clip).
                    kp_h: ROOT_SUPPORT.kp_h * (0.2 + 0.8 * up_now) * RISE_FLAT.2,
                    kd_h: ROOT_SUPPORT.kd_h * (0.2 + 0.8 * up_now) * RISE_FLAT.3,
                    max_force_h: (250.0 + 650.0 * up_now) * k * if upright { 2.0 } else { 1.0 },
                    kp_turn: ROOT_SUPPORT.kp_turn * 0.5,
                    kd_turn: self.root_kd_turn * 0.5,
                    max_torque: 300.0 * k,
                });
                // Upright, the torso's own flat sway damped toward the ground's too (the pelvis
                // held, the chest swung on: the centre never still). Not before: it held the
                // torso back from coming over the feet.
                let torso = b.torso;
                // Less while the torso leans past [`TORSO_STILL_FULL`] (none at [`STILL_TILT_OUT`]): it held
                // a leaning torso's centre back from coming over the pelvis, and a body leaning
                // up a slope never straightened to stand.
                let lean_left = ((STILL_TILT_OUT.to_radians() - reading.tilt)
                    / (STILL_TILT_OUT - TORSO_STILL_FULL).to_radians())
                .clamp(0.0, 1.0);
                if upright {
                    pulls.push(Support {
                        body: torso,
                        x: self.states[torso].x,
                        v: [self.ground_v[0], self.ground_v[1], 0.0],
                        q: self.states[torso].q,
                        kp: 0.0,
                        kd: 0.0,
                        max_force: 0.0,
                        kp_h: 0.0,
                        kd_h: TORSO_STILL.0 * lean_left,
                        max_force_h: TORSO_STILL.1 * k * lean_left,
                        kp_turn: 0.0,
                        kd_turn: 0.0,
                        max_torque: 0.0,
                    });
                }
            }
            // Not getting up (the pelvis no higher for a while): give up and settle back (the
            // AI may try again).
            if down.rise_stall > crate::np_down::RISE_STALL {
                self.rise = false;
                down.rise_failed = true;
                down.rising = 0.0;
                down.rise_best = 0.0;
                down.rise_landed = false;
                down.rise_mark = 0.0;
                down.arms_off = false;
                down.arms_off = false;
                down.rise_stall = 0.0;
            }
        } else if rising_wish && !rising {
            self.rise = false;
            down.rise_failed = true;
            down.no_push = 0.0;
            down.rising = 0.0;
            down.rise_best = 0.0;
            down.rise_landed = false;
            down.rise_mark = 0.0;
            down.arms_off = false;
        } else if !rising_wish {
            down.rising = 0.0;
            down.rise_best = 0.0;
            down.rise_landed = false;
            down.rise_mark = 0.0;
            down.arms_off = false;
        }
        // Not rising: firing from the ground the weapon arm aims (the game's) and the free arm
        // braces; else no hand stays planted.
        if !(rising && !airborne) {
            let brace: Vec<usize> = match (self.firing, self.weapon_side) {
                (true, Some(w)) => vec![1 - w],
                _ => Vec::new(),
            };
            let worlds = self.model.node_worlds(&self.states);
            let mut posed = worlds.clone();
            let facing = reading.facing;
            self.plant_hands(
                &mut down, &brace, &worlds, floor, facing, &mut posed, &mut pulls,
            );
            if !brace.is_empty() {
                let rel = relative_of(&self.model, &posed);
                self.drive_planted_arms(&down, &rel, &mut targets, &mut gains, &mut holds);
            }
        }
        // At most [`RISE_MOST`]: then it gives up (the AI waits, or gives up into the game's
        // ragdoll after its tries); never while it is up and moving (the push let go of an
        // upright body moving at 0.4 m/s and it tripped onto its front, Patrick's 22:19 clip):
        // then it is brought still (at most [`STILL_MOST`] more) and the balance takes it.
        // (Upright as the still stage holds it too: read alone, its feet's contact flickered,
        // a body standing still timed out and was let go, and it fell.)
        let upright = classify(&reading, false) == Label::Standing || down.still_up;
        // Going up at the end of its time (the pelvis rising at 1.3 m/s through 0.72 m when the
        // time ran out, and the rise was given up): its time runs on as the still stage's does.
        let going_up = reading.com_v[2] > RISE_GOING_UP
            || reading.pelvis_height >= RISE_NEARLY_UP * reading.stand_height;
        let over = down.rising > RISE_MOST && down.rising <= RISE_MOST + STILL_MOST;
        if self.rise && over && upright {
            down.still_wait = true;
        } else if self.rise && over && going_up {
            // Still going up: the rise goes on.
        } else if self.rise && down.rising > RISE_MOST {
            self.rise = false;
            down.rise_failed = true;
            down.rising = 0.0;
            down.rise_best = 0.0;
            down.rise_landed = false;
            down.rise_mark = 0.0;
            down.arms_off = false;
            down.rise_stall = 0.0;
            down.front_stage = 0;
            down.front_done = false;
        }
        let drives: Vec<Drive> = targets
            .iter()
            .zip(gains)
            .zip(holds)
            .map(|((&target, (kp, kd)), hold)| Drive {
                target,
                kp,
                kd,
                hold,
            })
            .collect();
        let tracking = Tracking {
            w: vec![[0.0; 3]; self.states.len()],
            rate: vec![0.0; self.states.len()],
            slip: Vec::new(),
            // Lifting, the feet carry the push.
            grip: {
                let mut g = if down.lifting {
                    b.feet.to_vec()
                } else {
                    Vec::new()
                };
                for side in 0..2 {
                    if down.hand_plants[side].is_some() {
                        g.push(b.arms[side][2]);
                    }
                }
                g
            },
            before: std::mem::replace(&mut self.last_targets, anim_rel.clone()),
            widen_by: anim_rel,
            reach: b.reach.clone(),
            // Soft only just downed, while the limits change from the standing ones (widened by
            // the legs' footing) to the downed ones (the animation's and the reach, which stay);
            // hard otherwise.
            soft_limits: down.since < LIMITS_SETTLE || self.to_limits,
            allow: Vec::new(),
            floors: self.floors.clone(),
            end_floors: self.end_floors.clone(),
            edges: self.edges.clone(),
            data_limits: self.to_limits,
            body_walls: self.body_walls.clone(),
            body_wall_tops: self.body_wall_tops.clone(),
        };
        let tracking = self.with_allowance(tracking);
        self.report = self.model.step_world(
            &mut self.states,
            &drives,
            &pulls,
            Some(&tracking),
            f,
            &self.walls,
            dt,
        );
        self.limit_allow = std::mem::take(&mut self.report.allow);
        // Standing again, balanced over its feet: the balance has it.
        let mut now = read(&self.model, &b, &self.states, &self.report, f);
        if down.lifting {
            now.pushed = (now.weight - now.total).max(0.0);
        }
        // Standing again only still: its centre's flat speed against the ground under
        // [`STAND_STILL`] (0.4 m/s handed the balance a body that leant, drifted and tripped:
        // Patrick's 22:19 clip), or upright past the rise's time and brought as still as it
        // gets ([`STILL_MOST`]).
        let rel = sub(now.com_v, self.ground_v);
        let flat_speed = crate::flinch::length([rel[0], rel[1], 0.0]);
        // Standing: as read, or held up for the still stage ([`up_for_still`]; the reading's feet
        // came and went frame to frame on a body standing still, and each frame they went the
        // stand's hold started over).
        let standing = classify(&now, false) == Label::Standing || down.still_up;
        // And its centre over its soles (accepted beside them, on a slope, the balance took a
        // catching step at once).
        // And its spine as the animation stands it (the torso against the pelvis within
        // [`STAND_POSE`] of the animation's).
        let posed_as_anim = self.posed_as_anim();
        // And its feet in the animation's stance (else its balance would step at once).
        // (Done once the gap is under [`STANCE_THERE`]; then off again only past [`STANCE_KEPT`]:
        // fixed again at its line, the foot came up and down and the stand never held.)
        let in_stance = down.stance_fixing.is_none() && self.stance_apart(anim).0 <= STANCE_KEPT;
        let over = posed_as_anim && in_stance && now.over_feet < STAND_INSIDE && {
            // ... as the balance taking it will find it (its own soles, each foot planted on its
            // own ground).
            let mut fresh = Balance::new(&self.model);
            for side in 0..2 {
                fresh.foot_floor[side] = self.floors.get(fresh.feet[side]).copied();
            }
            fresh.would_find(&self.model, &self.states, f) < STAND_INSIDE
        };
        if standing && ((flat_speed < STAND_STILL && over) || down.rising > RISE_MOST + STILL_MOST)
        {
            down.stood += dt;
        } else {
            down.stood = 0.0;
        }
        // The actor goes where the body is: the animation follows the pelvis.
        let pelvis = self.states[r].x;
        let anim_root = scale(anim[r].1, 1.0 / UNITS);
        self.balance.shift = [pelvis[0] - anim_root[0], pelvis[1] - anim_root[1], 0.0];
        if down.stood >= STAND_HOLD {
            // The animation on the feet as they stand (its feet's middle on theirs; the root on
            // the pelvis, the balance's own shift slid it there after the hand-over, the body
            // and the reference with it).
            let fb = self.balance.feet;
            let unshifted = self.model.states_from_nodes(anim);
            let body_mid = scale(add(self.states[fb[0]].x, self.states[fb[1]].x), 0.5);
            let anim_mid = scale(add(unshifted[fb[0]].x, unshifted[fb[1]].x), 0.5);
            let shift = [body_mid[0] - anim_mid[0], body_mid[1] - anim_mid[1], 0.0];
            self.balance = Balance::new(&self.model);
            self.balance.shift = shift;
            // Both feet planted where they stand (the balance's legs reach them, as after a
            // step's landing, until a stance step brings them to the animation's stance): driven
            // to the animation's own legs once standing, the pelvis was pushed off where it
            // stood.
            let feet = self.balance.feet;
            for side in 0..2 {
                let st = self.states[feet[side]];
                self.balance.plants[side] = Some((st.x, st.q));
            }
            // Recovering first, as after a step: no step of its own while the pose eases into
            // the animation's ([`STAND_BLEND`]).
            self.balance.phase = crate::np_balance::Phase::Recovering;
            // Handed over at rest, as the game's own get-up leaves a body (its bones and
            // controller stilled): no flat motion against the ground left in the bodies, and the
            // animation's measured velocity the ground's.
            let g = self.ground_v;
            for st in self.states.iter_mut() {
                st.v = [g[0], g[1], st.v[2]];
            }
            self.anim_v = [g[0], g[1], 0.0];
            self.anim_samples.clear();
            self.stand_from = Some((
                relative_of(&self.model, &self.model.node_worlds(&self.states)),
                self.states[r].q,
            ));
            self.rise = false;
            self.down = None;
            self.stood_up = 0.0;
        } else {
            self.down = Some(down);
        }
        self.model.node_worlds(&self.states)
    }

    /// How far the feet stand off the animation's stance (m, flat: each foot's offset from its
    /// animated place, the one against the other), the foot carrying less of the body (further
    /// from its centre, flat) and that foot's node's place in the stance beside the other (game
    /// units, flat).
    fn stance_apart(&self, anim: &[(M3, V3)]) -> (f32, usize, V3) {
        let fb = self.bodies.feet;
        let shift = self.balance.shift;
        let flat = |a: V3| [a[0], a[1], 0.0];
        let worlds = self.model.node_worlds(&self.states);
        let anim_at = |side: usize| add(anim[fb[side]].1, scale(shift, UNITS));
        let off = |side: usize| flat(sub(worlds[fb[side]].1, anim_at(side)));
        let apart = crate::flinch::length(sub(off(0), off(1))) / UNITS;
        let (com, _) = self.model.centre_of_mass(&self.states);
        let from_com = |side: usize| crate::flinch::length(flat(sub(self.states[fb[side]].x, com)));
        let side = if from_com(0) > from_com(1) { 0 } else { 1 };
        (apart, side, flat(add(anim_at(side), off(1 - side))))
    }

    /// Whether the torso stands against the pelvis within [`STAND_POSE`] of the animation's.
    fn posed_as_anim(&self) -> bool {
        let (r, tb) = (self.model.root, self.bodies.torso);
        let rel = |s: &[State]| qnorm(qmul(qconj(s[r].q), s[tb].q));
        let d = crate::active::qlog(qmul(rel(&self.anim_states), qconj(rel(&self.states))));
        crate::flinch::length(d) <= STAND_POSE.to_radians()
    }

    /// `side`'s foot's node's place in the animation's stance beside the other foot (game units,
    /// flat).
    fn stance_place(&self, anim: &[(M3, V3)], side: usize) -> V3 {
        let fb = self.bodies.feet;
        let shift = scale(self.balance.shift, UNITS);
        let worlds = self.model.node_worlds(&self.states);
        let o = 1 - side;
        let off = sub(worlds[fb[o]].1, add(anim[fb[o]].1, shift));
        let at = add(add(anim[fb[side]].1, shift), off);
        [at[0], at[1], 0.0]
    }

    /// Each arm of `arms` put down on the ground (the rise's push, or the free arm's brace while
    /// the weapon arm aims): its hand set down beside its shoulder, a little toward `dir`, and
    /// held where it lands (no slide, no bounce: guided down with a hand's reach, then the ground
    /// alone holds it up); the arm's pose for it into `posed`, driven straight under a shoulder
    /// raised by half the arm, so it pushes the torso up. Any other arm lets go.
    #[allow(clippy::too_many_arguments)]
    fn plant_hands(
        &self,
        down: &mut Down,
        arms: &[usize],
        worlds: &[(M3, V3)],
        floor: f32,
        dir: V3,
        posed: &mut [(M3, V3)],
        pulls: &mut Vec<Support>,
    ) {
        let b = &self.bodies;
        let k = self.effort.clamp(0.0, 1.0);
        for side in 0..2 {
            if !arms.contains(&side) {
                down.hand_plants[side] = None;
                down.hand_landed[side] = false;
                continue;
            }
            let [upper, fore, hand] = b.arms[side];
            let (shoulder, elbow, wrist) = (worlds[upper].1, worlds[fore].1, worlds[hand].1);
            let torso_at = worlds[b.torso].1;
            let out =
                crate::flinch::unit([shoulder[0] - torso_at[0], shoulder[1] - torso_at[1], 0.0])
                    .unwrap_or([0.0; 3]);
            let hand_def = &self.model.bodies[hand];
            let plant = *down.hand_plants[side].get_or_insert_with(|| {
                let xy = add(
                    add(shoulder, scale(out, HAND_OUT * UNITS)),
                    scale(dir, HAND_AHEAD * UNITS),
                );
                // On the body's side of a wall beside it, the hand's radius and a little more
                // off its face (a spot beside the shoulder lay in the wall: the arm drove the
                // hand into it at 200 N, holding the torso 30 degrees over through the rise).
                let walls = self.walls.iter().chain(self.body_walls.iter().flatten());
                let xy = walls.fold(xy, |at, w| {
                    if w.n[2].abs() > HAND_WALL_FLOOR {
                        return at;
                    }
                    let gap = dot(sub(scale(at, 1.0 / UNITS), w.p), w.n);
                    let need = hand_def.radius + HAND_WALL;
                    if gap >= need || gap < -HAND_WALL_BEHIND {
                        return at;
                    }
                    let n = [w.n[0], w.n[1], 0.0];
                    add(at, scale(n, (need - gap) * UNITS))
                });
                let z =
                    self.ground_at(hand, xy).map_or(floor, |g| g * UNITS) + hand_def.radius * UNITS;
                [xy[0], xy[1], z]
            });
            let arm = crate::flinch::length(sub(elbow, shoulder))
                + crate::flinch::length(sub(wrist, elbow));
            let raised = add(shoulder, [0.0, 0.0, 0.5 * arm]);
            if let Some((r1, m, _)) =
                crate::flinch::leg_ik(shoulder, elbow, wrist, raised, plant, scale(out, -1.0))
            {
                let mr1 = crate::flinch::mul(&m, &r1);
                posed[upper].0 = crate::flinch::mul(&r1, &worlds[upper].0);
                posed[fore].0 = crate::flinch::mul(&mr1, &worlds[fore].0);
            }
            let q = self.states[hand].q;
            let x = add(
                scale(plant, 1.0 / UNITS),
                crate::active::qrot(q, hand_def.com),
            );
            // Brought down onto its spot until it touches; then the ground pushes it (a pull
            // down held the torso down as the arm straightened).
            let landed = down.hand_landed[side]
                || self.report.floor_load.get(hand).copied().unwrap_or(0.0) > 1.0;
            down.hand_landed[side] = landed;
            pulls.push(Support {
                body: hand,
                x,
                v: [0.0; 3],
                q,
                kp: if landed { 0.0 } else { HAND_PLANT.0 },
                kd: if landed { 0.0 } else { HAND_PLANT.1 },
                max_force: HAND_REACH * k,
                kp_h: HAND_PLANT.0,
                kd_h: HAND_PLANT.1,
                max_force_h: if landed { HAND_PLANT.2 } else { HAND_REACH } * k,
                kp_turn: 0.0,
                kd_turn: 0.0,
                max_torque: 0.0,
            });
        }
    }

    /// A planted arm's joints driven toward its pose in `posed_rel`, as far as a frame's effort
    /// goes (whole, the arm's goal yanked it: the pelvis turned 20 degrees in a frame as the rise
    /// began).
    fn drive_planted_arms(
        &self,
        down: &Down,
        posed_rel: &[Q],
        targets: &mut [Q],
        gains: &mut [(f32, f32)],
        holds: &mut [f32],
    ) {
        let k = self.effort.clamp(0.0, 1.0);
        let now = self.model.relative(&self.states);
        for side in 0..2 {
            if down.hand_plants[side].is_none() {
                continue;
            }
            for (j, def) in self.model.joints.iter().enumerate() {
                if self.bodies.arms[side][..2].contains(&def.child) {
                    targets[j] = turn_toward(now[j], posed_rel[j], RISE_REACH);
                    gains[j].0 = self.muscles.base[j].0 * RISE_GAIN * k;
                    holds[j] = 0.5 * k;
                }
            }
        }
    }

    /// The arms that push in a rise: unarmed both; armed the free arm, the weapon arm too only
    /// when the free arm finds no ground within its reach (a wall's foot, a ledge).
    fn pushing_arms(&self, worlds: &[(M3, V3)], floor: f32) -> Vec<usize> {
        let b = &self.bodies;
        let Some(w) = self.weapon_side else {
            return vec![0, 1];
        };
        let free = 1 - w;
        let [upper, fore, hand] = b.arms[free];
        let reach = crate::flinch::length(sub(worlds[fore].1, worlds[upper].1))
            + crate::flinch::length(sub(worlds[hand].1, worlds[fore].1));
        let shoulder = worlds[upper].1;
        let ground = self.ground_at(hand, shoulder).map_or(floor, |g| g * UNITS);
        // The weapon's hand only where its own ground is in its reach and the free hand's is not
        // (a ledge's or a wall's foot under the free side); on flat ground both are as far, and
        // the weapon's hand stays off it.
        let [w_upper, _, w_hand] = b.arms[w];
        let w_shoulder = worlds[w_upper].1;
        let w_ground = self
            .ground_at(w_hand, w_shoulder)
            .map_or(floor, |g| g * UNITS);
        if shoulder[2] - ground <= reach * 1.05 || w_shoulder[2] - w_ground > reach * 1.05 {
            vec![free]
        } else {
            vec![free, w]
        }
    }

    /// Body `b`'s own ground under `at` (game units, local; metres answered), from its floor
    /// plane ([`Np::floors`]); none without one.
    fn ground_at(&self, b: usize, at: V3) -> Option<f32> {
        let p = self.floors.get(b)?;
        if p.n[2] <= 0.1 {
            return None;
        }
        let (x, y) = (at[0] / UNITS, at[1] / UNITS);
        Some(p.p[2] - (p.n[0] * (x - p.p[0]) + p.n[1] * (y - p.p[1])) / p.n[2])
    }

    /// Whether the rise holds the body up for its still stage ([`up_for_still`]'s latch).
    pub fn still_up(&self) -> bool {
        self.down.as_ref().is_some_and(|d| d.still_up)
    }

    /// The pose's label now (none while standing on its feet under the balance).
    pub fn label(&self) -> Option<Label> {
        self.down.as_ref().map(|d| d.labeler.label)
    }

    /// The animated pose (`anim`, shifted, game units) with its legs bent onto the balance's
    /// footing; none while the balance is not engaged (the animation's own legs).
    fn legs_on_footing(
        &self,
        anim: &[(M3, V3)],
        ask: &crate::np_balance::Ask,
    ) -> Option<Vec<(M3, V3)>> {
        let b = &self.balance;
        // How far each animated foot lies under its own ground (m; a flat animation's stance on
        // a slope puts the uphill foot into it, and the legs drove it there at 4 to 5 body
        // weights).
        let under = [0, 1].map(|side| {
            let f = b.feet[side];
            let (Some(p), Some(s)) = (b.foot_floor[side], self.anim_states.get(f)) else {
                return 0.0;
            };
            if p.n[2] <= 0.1 {
                return 0.0;
            }
            let def = &self.model.bodies[f];
            def.ends
                .iter()
                .map(|e| {
                    let end = add(s.x, crate::active::qrot(s.q, sub(*e, def.com)));
                    let ground =
                        p.p[2] - (p.n[0] * (end[0] - p.p[0]) + p.n[1] * (end[1] - p.p[1])) / p.n[2];
                    ground - (end[2] - def.radius)
                })
                .fold(0.0f32, f32::max)
        });
        let into_ground = under.iter().any(|&u| u > 0.01);
        // How far each animated foot lies in a wall below its top, and the wall's way out (a
        // walker kept walking into a slab's face: the animation's foot drove into it at 3700 N
        // and lay 5.6 u inside).
        let in_wall = [0, 1].map(|side| {
            let f = b.feet[side];
            let (Some(s), Some(walls)) = (self.anim_states.get(f), self.body_walls.get(f)) else {
                return None;
            };
            let tops = self.body_wall_tops.get(f);
            let def = &self.model.bodies[f];
            let mut deepest: Option<(f32, V3)> = None;
            for (k, w) in walls.iter().enumerate() {
                for e in &def.ends {
                    let end = add(s.x, crate::active::qrot(s.q, sub(*e, def.com)));
                    if tops.and_then(|t| t.get(k)).is_some_and(|&top| end[2] > top) {
                        continue;
                    }
                    let depth = def.radius - crate::flinch::dot(sub(end, w.p), w.n);
                    if depth > deepest.map_or(WALL_FOOT_IN, |d| d.0) {
                        deepest = Some((depth, w.n));
                    }
                }
            }
            deepest
        });
        let into_wall = in_wall.iter().any(|w| w.is_some());
        let engaged = into_ground
            || into_wall
            || if b.moving {
                // On its path the walk's legs drive; a gather step or a plant takes them.
                b.step.is_some() || b.plants.iter().any(|p| p.is_some())
            } else {
                b.phase != Phase::Standing
                    || b.plants.iter().any(|p| p.is_some())
                    || b.lower > 0.003
            };
        if !engaged || ask.falling {
            return None;
        }
        let body = |n: &str| self.model.body(n);
        let node_of = |bi: usize, x: V3, q: Q| -> V3 {
            let com = self.model.bodies[bi].com;
            scale(sub(x, crate::active::qrot(q, com)), UNITS)
        };
        let mut out = anim.to_vec();
        let hips = [body("Bip01 L Thigh")?, body("Bip01 R Thigh")?];
        let across = sub(anim[hips[1]].1, anim[hips[0]].1);
        let forward = crate::flinch::unit(crate::flinch::cross([0.0, 0.0, 1.0], across))?;
        for (side, prefix) in ["Bip01 L", "Bip01 R"].iter().enumerate() {
            let (thigh, calf, foot) = (
                body(&format!("{prefix} Thigh"))?,
                body(&format!("{prefix} Calf"))?,
                body(&format!("{prefix} Foot"))?,
            );
            let goal = match (ask.foot, ask.plants[side]) {
                (Some((fb, x, q, _)), _) if fb == foot => node_of(foot, x, q),
                (_, Some((_, x, q))) => node_of(foot, x, q),
                // An animated foot under its ground: where the animation has it, on the ground.
                _ if under[side] > 0.01 => {
                    let s = &self.anim_states[foot];
                    node_of(foot, add(s.x, [0.0, 0.0, under[side]]), s.q)
                }
                // An animated foot in a wall: where the animation has it, out of the wall.
                _ if in_wall[side].is_some() => {
                    let s = &self.anim_states[foot];
                    let (depth, n) = in_wall[side].unwrap_or((0.0, [0.0; 3]));
                    let out = crate::flinch::unit([n[0], n[1], 0.0]).unwrap_or([0.0; 3]);
                    node_of(foot, add(s.x, scale(out, depth)), s.q)
                }
                _ => node_of(foot, self.states[foot].x, self.states[foot].q),
            };
            let hip = sub(anim[thigh].1, [0.0, 0.0, ask.lower * UNITS]);
            let Some((r1, m, _level)) = crate::flinch::leg_ik(
                anim[thigh].1,
                anim[calf].1,
                anim[foot].1,
                hip,
                goal,
                forward,
            ) else {
                continue;
            };
            let mr1 = crate::flinch::mul(&m, &r1);
            out[thigh] = (crate::flinch::mul(&r1, &anim[thigh].0), hip);
            out[calf].0 = crate::flinch::mul(&mr1, &anim[calf].0);
            // The foot keeps the animation's world rotation (level).
            out[foot].0 = anim[foot].0;
        }
        Some(out)
    }

    /// The body brought inside the skeleton's own limits at once (`Model::into_data_limits`) and
    /// its bodies' nodes' worlds (game units, the world's): the pose handed to the game's ragdoll.
    /// Answers them and how far out the body still lay (rad).
    /// Each joint's angle out of the skeleton's own limits now (rad, the joints' order).
    pub fn joints_out(&self) -> Vec<f32> {
        self.model.data_limits_out(&self.states)
    }

    pub fn into_limits(&mut self) -> (Vec<(M3, V3)>, f32) {
        let out = self.model.into_data_limits(&mut self.states, 12);
        (
            shift_by(&self.model.node_worlds(&self.states), self.origin),
            out,
        )
    }

    /// Of the move [`Np::take_move`] answered, `by` (m, flat) was not made (the actor met a wall):
    /// the body stays that far off its animation again.
    pub fn give_back(&mut self, by: V3) {
        let back = scale(by, -1.0);
        self.balance.take(back);
        self.anim_moved(back);
    }

    /// The actor was moved by `by` (m; its animation comes moved by it, which is not its own
    /// motion nor a jump): a step's or a rise's end, and the height its owner gave it there.
    pub fn anim_moved(&mut self, by: V3) {
        if let Some(c) = self.anim_com.as_mut() {
            *c = add(*c, by);
        }
        for s in self.anim_samples.iter_mut() {
            s.1 = add(s.1, by);
        }
    }

    /// The way the body faces (its torso's forward, flat, unit).
    pub fn facing(&self) -> V3 {
        let f = self.bodies.forward_of(&self.states, self.bodies.torso);
        crate::flinch::unit([f[0], f[1], 0.0]).unwrap_or([0.0, 1.0, 0.0])
    }

    /// The animation's velocity (m/s) the last frame measured.
    pub fn anim_velocity(&self) -> V3 {
        self.anim_v
    }

    /// The animation's velocity (game units a second) before its first frame (an actor already
    /// walking when it is taken): the bodies move with it.
    pub fn set_anim_velocity(&mut self, v: V3) {
        self.anim_v = scale(v, 1.0 / UNITS);
        self.anim_samples.clear();
        for s in &mut self.states {
            s.v = self.anim_v;
        }
    }

    /// What the balance is doing.
    pub fn phase(&self) -> Phase {
        self.balance.phase
    }

    /// The body's move against its animation that the actor should make now (game units, flat):
    /// all of it once no step swings; the controller then expects the animation moved by it (the
    /// actor's reference and its 3D's root).
    pub fn take_move(&mut self) -> V3 {
        // Downed, the actor stays where it is; the body's offset is taken once it stands again
        // (the actor moved with the pelvis every frame glided 14 to 53 units a second: the
        // tester's 08:13 clip).
        if self.down.is_some()
            || self.balance.step.is_some()
            || self.balance.phase == Phase::Falling
        {
            return [0.0; 3];
        }
        let mut by = self.balance.shift;
        if crate::flinch::length(by) < 1e-4 {
            return [0.0; 3];
        }
        // Just stood up, the actor goes to the body at most at [`STAND_TAKE`] (taken whole, the
        // actor's capsule hopped 0.5 to 1 unit in a frame as the balance took the body: its
        // reference had followed the pelvis while down, the animation now stands on the feet);
        // the rest stays in the shift, taken the next frames.
        if self.stood_up < STAND_TAKE.1 {
            let most = STAND_TAKE.0 * self.last_dt;
            let l = crate::flinch::length(by);
            if l > most {
                by = scale(by, most / l);
            }
        }
        self.balance.take(by);
        self.anim_moved(by);
        scale(by, UNITS)
    }
}
