//! The lab's living body: the game's controller run on the capsule mannequin.
//!
//! **The same code as the game.** Everything the game's controller computes with is
//! `ragdoll_core`: the springs and their roles, the kick's spread and weights, the capture point,
//! the step plan, the ground classes, the two-bone IK, the stepping state, the brace's timing, the
//! momentum transfer, the stack's fading, the topple speed, the reaction and the launch. What is
//! here is the game's glue run on the mannequin instead of the game's bones, written line for line
//! after the host's `crate::active_ragdoll` (`hit`, `apply`, `balance_frame`, `push`,
//! `start_brace`, `reach_arm`, `brace_frame`, `fall_frame`, `fall`) and `crate::knockdown`
//! (`after_hit`, `knock_down`, the get-up), with the game's memory reads replaced by the
//! mannequin's pose and the navmesh by [`crate::world::ground_point`].
//!
//! **What differs.** The standing body's animation is the mannequin's rest pose (the game's idle
//! sways a little). The ragdoll is Rapier's, not Havok's (see `crate::mannequin` for its masses
//! and limits). The get-up is a blend back to the rest pose over [`GET_UP`] seconds (the game plays
//! its get-up animation). A hit on a body already in its ragdoll pushes the struck bone with the
//! hit's momentum (the game's knockdowns leave a downed body to the game's own hit impulse).

use crate::convert::{from_m, m3_of, quat_of, to_m, vec};
use crate::mannequin::{self as mq, COUNT};
use crate::weapons::{self, Weapon};
use crate::world::{self, Ground, GroundKind};
use ragdoll_core::active;
use ragdoll_core::balance::{plan, Limbs, Outcome, Stepping};
use ragdoll_core::brace::{self as hands, Hand, Surface};
use ragdoll_core::condition::{self, Condition};
use ragdoll_core::flinch::*;
use ragdoll_core::knock::{
    add_hit, body_mass, launch_direction, launch_speed, reaction, topple_speed, transferred, Body,
    Push, Reaction, HOLD_SECONDS, LYING_HEIGHT, STILL_FRAMES, STILL_SPEED,
};
use ragdoll_core::obstacle;
use ragdoll_core::perks;
use ragdoll_core::tuning::{self, Tuning};
use rapier3d::prelude::*;

/// The simulation's step (s): the game's frame at 60 frames a second.
pub const STEP: f32 = 1.0 / 60.0;
/// No second knock-down this soon after one, nor this soon after getting up (s): the host's
/// `crate::knockdown::KNOCK_COOLDOWN` and `UP_COOLDOWN`.
pub const KNOCK_COOLDOWN: f64 = 2.5;
pub const UP_COOLDOWN: f64 = 1.5;
/// The flight's bone speed and spin bounds and how long they hold (the host's `MAX_BONE_SPEED`,
/// `MAX_BONE_SPIN`, `FLIGHT`).
pub const MAX_BONE_SPEED: f32 = 1400.0;
pub const MAX_BONE_SPIN: f32 = 20.0;
pub const FLIGHT: f64 = 1.5;
/// The feet go back from where the steps left them to the animation's over this long (s), eased
/// (the host's `active_ragdoll::FEET_BACK`).
pub const FEET_BACK: f32 = 0.4;
/// The least of the support's carrying while the posed body goes over (the host's), and how long
/// a fall that did not happen takes to ease back up (s).
pub const FALL_SUPPORT: f32 = 0.5;
pub const FALL_BACK: f32 = 0.6;
/// How much deeper than the animation (game units) a shown forearm or hand may be in the torso
/// before its arm is turned out (`active::Body::clear_arms`; the host's `ARM_SLACK`).
pub const ARM_SLACK: f32 = 0.35;
/// How long the steps' pelvis offset takes back to none once they are done (s; the host's).
pub const SHIFT_BACK: f32 = 0.3;
/// The get-up's blend back to the rest pose (s).
pub const GET_UP: f32 = 1.6;
/// How far from the body the scripted and clicked shots come (m), and how wide a shotgun's
/// pattern grows a metre (units of spread radius per metre of range).
pub const PELLET_SPREAD: f32 = 2.4;
/// How far ahead of the shoulders a brace looks for its wall (units; the host's `BRACE_PROBE`).
pub const BRACE_PROBE: f32 = 90.0;
/// An armed body's weapon arm held on its aim while its free arm braces: its motors this many
/// times firmer (the active body's dip below 0).
pub const AIM_FIRM: f32 = 4.0;
/// The way the mannequin faces (+Y; the host reads its actor's 3D root).
pub const FORWARD: V3 = [0.0, 1.0, 0.0];
/// How long the gait takes in and out (s).
pub const GAIT_IN: f32 = 0.3;
/// The share of the way back to straight a ragdoll elbow or knee bent backwards is turned each
/// step ([`Sim::stop_ragdoll_hinges`]).
pub const RAGDOLL_HINGE_SHARE: f32 = 1.0;

/// Hands reaching to a wall along `dir` (stage 3), `t` seconds in.
#[derive(Clone, Copy, Debug)]
pub struct Brace {
    pub dir: V3,
    pub t: f32,
    /// How far ahead the obstacle is (units; ray casts at the brace's start).
    pub wall: Option<f32>,
    /// A waist-high obstacle's top (world z, units): the hands go on it.
    pub top: Option<f32>,
    /// What the hands meet, and each bracing hand (`ragdoll_core::brace`; none: that arm does not
    /// brace: missing, or an armed body's weapon arm keeping its aim).
    pub surface: Option<Surface>,
    pub hands: [Option<Hand>; 2],
    /// The animation's facing when the brace began, and how far the actor was walked since (its
    /// hands let go once it turns or is walked away: `ragdoll_core::brace::let_go`).
    pub facing: V3,
    pub walked: f32,
}

impl Brace {
    /// The brace's weight on an arm's targets (how far its reach is in, 0 to 1).
    pub fn weight(&self, side: usize) -> f32 {
        self.hands[side].map_or(0.0, |h| h.reach_weight(self.t))
    }

    /// The most any hand's reach is in.
    pub fn any_weight(&self) -> f32 {
        self.weight(0).max(self.weight(1))
    }

    /// Whether an armed body's weapon arm stays on its aim (it does not brace).
    pub fn aims(&self, side: usize, armed: bool) -> bool {
        armed && side == hands::WEAPON_SIDE as usize && self.hands[side].is_none()
    }
}

/// The fall into the ragdoll under way (stage 3; the host's `active_ragdoll::Fall`).
#[derive(Clone, Copy, Debug)]
pub struct Fall {
    pub dir: V3,
    pub speed: f32,
    pub point: V3,
    pub t: f32,
    pub lead: f32,
    pub windmill: bool,
    pub why: &'static str,
    /// Folding over a waist-high obstacle: its top (world z, units).
    pub fold: Option<f32>,
}

/// The rig (the host's `active_ragdoll::Rig`): each bone's spring, the pelvis's drop, the steps,
/// the brace and the fall.
#[derive(Clone, Debug)]
pub struct Rig {
    /// The hits' flinches drawn on the torso (`active::TorsoFlinch`).
    pub flinches: Vec<active::TorsoFlinch>,
    /// How much of the body's own torso turn is drawn (`active::torso_keep`).
    pub torso_keep: f32,
    pub springs: [Spring; COUNT],
    pub drop: f32,
    pub drop_speed: f32,
    pub since: f64,
    pub balance: Option<Stepping>,
    pub shift: V3,
    pub limbs: Limbs,
    pub brace: Option<Brace>,
    pub fall: Option<Fall>,
    pub active: bool,
    /// The arms' shown pose offsets following the poses they are given (the host's
    /// `Bone::follow`), and how much of its aim an armed body keeps now.
    pub follow: [Follow; COUNT],
    pub aim_hold: f32,
    /// The physical body while it reacts (`ragdoll_core::active`), when it was last hit.
    pub body: Option<active::Body>,
    pub hit_at: f64,
    /// Where the feet stand (the pins' targets between steps), the steps' last targets.
    pub planted: Option<([V3; 2], f64)>,
    /// Why the body was dropped back to the animation (the game's `active_ragdoll` does the same).
    pub failed: Option<String>,
    pub held: Vec<Option<M3>>,
    /// The host's `Rig::origin`, `fade`, `last` and `shift_back`: the body runs in metres from
    /// where it was made; a dropped body's last shown pose eases out over the animation
    /// (`ragdoll_core::active::Fade`); the steps' pelvis offset eases out once they are done.
    pub origin: V3,
    pub fade: Option<active::Fade>,
    pub last: Option<(Vec<active::Q>, V3)>,
    pub shift_back: Option<(V3, f64, f32)>,
    /// After a fall that did not happen: the support it had then, and since when it eases back.
    pub support_back: Option<(f32, f64)>,
    /// The hand-back only settles (`ragdoll_core::active::Settle`; the host's `Rig::settle`).
    pub settle: active::Settle,
    /// The shown offsets held to the push (`ragdoll_core::active::Limit`), the push (m/s, the
    /// most of the body's hits'), the body's weight in the shown pose last frame.
    pub limit: active::Limit,
    pub react_v: f32,
    pub shown_w: f32,
    /// When the reaction yielded to its animation (`ragdoll_core::active::yields`), and the AI's
    /// move (units) and turn (rad) of the actor since the last hit.
    pub yield_at: Option<f64>,
    pub moved_since_hit: f32,
    pub turned_since_hit: f32,
    /// Each bone's animated frame (world rotation, joint) the frame before: the body carried
    /// along with the animation's motion (`Body::carry`).
    pub carry_from: Option<Vec<(active::Q, V3)>>,
}

impl Rig {
    fn new(limbs: Limbs) -> Rig {
        Rig {
            flinches: Vec::new(),
            torso_keep: active::BODY_TORSO_SHARE,
            springs: [Spring::REST; COUNT],
            drop: 0.0,
            drop_speed: 0.0,
            since: 0.0,
            balance: None,
            shift: [0.0; 3],
            limbs,
            brace: None,
            fall: None,
            active: false,
            follow: [Follow::REST; COUNT],
            aim_hold: 0.0,
            body: None,
            hit_at: 0.0,
            planted: None,
            failed: None,
            held: Vec::new(),
            origin: [0.0; 3],
            fade: None,
            last: None,
            shift_back: None,
            support_back: None,
            settle: active::Settle::default(),
            limit: active::Limit::default(),
            react_v: 0.0,
            shown_w: 0.0,
            yield_at: None,
            moved_since_hit: 0.0,
            turned_since_hit: 0.0,
            carry_from: None,
        }
    }
}

/// A bone's world rotation and position.
pub type Xf = (M3, V3);

/// Standing, in its ragdoll, or getting up.
pub enum State {
    Standing,
    Ragdoll {
        bodies: Vec<Option<RigidBodyHandle>>,
        since: f64,
        still: u32,
        /// The fall's direction (flat) and whether the arms have braced for the ground.
        dir: V3,
        braced: bool,
    },
    GettingUp {
        from: Box<[Xf; COUNT]>,
        t: f32,
    },
}

/// The stack of impacts (the host's `knockdown` stack): momentum, its direction's vector, the
/// hits in it, when the last came.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stack {
    pub total: f32,
    pub vector: V3,
    pub hits: u32,
    pub at: f64,
}

/// What a run did (the headless reports and the page's results).
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub hits: u32,
    pub first_hit: Option<f64>,
    pub last_hit: Option<f64>,
    pub fell: bool,
    pub fall: Option<String>,
    pub steps: u32,
    pub braced: bool,
    pub flinches: u32,
    pub peak_speed: f32,
    pub topple: f32,
    pub decisions: Vec<String>,
    pub recovered_at: Option<f64>,
    pub start: V3,
    /// The fastest an arm bone turned while the body stood (rad/s), and in its ragdoll.
    pub arm_spin: f32,
    pub ragdoll_arm_spin: f32,
    /// The most an armed body's weapon hand turned off its aim (degrees), the most any bone was
    /// turned off its pose by the flinch (degrees), and whether an attack was interrupted.
    pub aim_drift: f32,
    pub flinch: f32,
    pub interrupted: bool,
    /// Smoothness while it stands (the shown pose): the biggest change of a bone's velocity in one
    /// frame (m/s) and the biggest jerk (m/s3).
    pub velocity_jump: f32,
    pub jerk: f32,
    /// How often the body was dropped back to the animation as not believable.
    pub dropped: u32,
    /// The most any elbow or knee was bent backwards, standing, in the ragdoll or getting up
    /// (degrees; [`back_bend`]).
    pub back_bend: f32,
    /// A swinging foot caught on a low obstacle; what the brace was on ("waist", "wall").
    pub tripped: bool,
    pub brace_kind: Option<String>,
    /// The brace's hands (`ragdoll_core::brace`, the shown pose): which arms braced; how often a
    /// hand lost the surface while it held (a bounce); the fastest a touching hand moved (m/s) and
    /// the furthest it slid from where it settled (cm); how often a planted arm's elbow turned
    /// back on itself by more than [`ELBOW_SWING`] (an oscillation; the worst arm's count).
    pub brace_hands: [bool; 2],
    pub hand_losses: u32,
    pub hand_jitter: f32,
    pub hand_slip: f32,
    /// The furthest a planted wrist was drawn from its plant once at rest there (units), and the
    /// longest a released hand took home (s; Patrick's 21-16-09 clip: 3 s).
    pub plant_drift: f32,
    pub return_time: f32,
    pub elbow_reversals: u32,
    /// A two-handed weapon's support hand going back to its grip after a brace (the shown pose,
    /// relative to the grip, which moves with the weapon hand): whether it was watched, how far
    /// it went past the grip along its way back (cm), how often its distance to the grip grew
    /// again by more than [`REGRIP_SWING`] (a wobble), and how far it came off the grip after it
    /// first got within [`REGRIP_LOCK`] (cm).
    pub regrip_watched: bool,
    pub regrip_overshoot: f32,
    pub regrip_reversals: u32,
    pub regrip_drift: f32,
    /// How far off a two-handed weapon's grip the support hand was each frame (0 on it, 1 fully
    /// off: `ragdoll_core::one_hand::off_grip`), from the first hit on: the one-handed accuracy
    /// penalty's share.
    pub off_grip: Vec<f32>,
    /// The hand-back (`ragdoll_core::active::Settle`, Patrick's c2a53c0 fold clip): the most the
    /// pelvis's shown offset from its animation grew in a frame while it settles (units; it only
    /// settles: 0), the biggest move of the shown pelvis in a frame then beyond the actor's own
    /// (units: no snap back), and, over the whole reaction, the fastest the shown pelvis turned
    /// against its animation (rad/s: no spin) and the most it was turned off it (rad).
    pub settle_rise: f32,
    pub settle_jump: f32,
    /// The most the pelvis's shown offset from its animation (units) grew past what it was when
    /// nothing held the body any more (its hands let go or never braced, its steps done, no fall,
    /// no hit since): Patrick's c2a53c0 clip, 3.5 to 32 units after "recovered".
    pub release_rise: f32,
    /// The host's read back of the animation ([`Sim::read_back`]): reads taken for a later pass's
    /// move (the remembered pose kept) and animated jumps eased in.
    pub base_stale: u32,
    pub base_jumps: u32,
    /// Over the reaction: the most the shown pelvis was off its animation (units), the biggest
    /// move of its offset in a frame (units, the steps' own moves of the actor aside), and the
    /// fastest any bone's offset from its animation turned (rad/s) and which.
    pub pelvis_most: f32,
    /// The floor (Patrick's 19-43-08 clip), standing: the most the shown pelvis was under its
    /// animation (units; before any step, and once a step was taken), a shown foot under its
    /// animation, a shown foot or the pelvis under the floor there, and the reference's height
    /// moved.
    pub pelvis_drop: f32,
    /// The most the torso (pelvis, spine, neck, head) was turned off its animation (degrees), and
    /// the body's push (the most of its hits', m/s).
    pub torso_turn: f32,
    /// The most that turn changed in a frame (degrees: the host's `drawn_turn` jumping).
    pub turn_jump: f32,
    /// The get-up (Patrick's 08-21-49 clip): whether the body lay when it was let up (none: never
    /// let up), its pelvis over the floor then (units), the nudges it took to topple, and the
    /// fastest bone through the get-up (units a second).
    pub got_up_lying: Option<bool>,
    pub got_up_pelvis: f32,
    pub nudges: u32,
    pub get_up_fastest: f32,
    /// Each frame from the first hit: the time since it (s), the spine's top turned off its
    /// animation (degrees), its twist about the vertical (degrees, signed), the head's turn.
    pub spine_trace: Vec<(f32, f32, f32, f32)>,
    /// Leg-only animation jumps at a hit re-based (`active::rebases`).
    pub rebased: u32,
    /// Continuity (Patrick's 21-16-09 clip): the most a drawn bone moved (units) and turned
    /// (degrees) in a frame beyond its animation's own motion, standing; which bone and when.
    pub drawn_move: f32,
    pub drawn_turn_step: f32,
    pub drawn_move_bone: usize,
    pub drawn_move_at: f64,
    pub drawn_turn_bone: usize,
    /// The farthest a drawn bone was from its animated place (units; a fall's and a bracing
    /// arm's aside), and the most a drawn bone's length was off the animation's (units).
    pub drawn_most_off: f32,
    pub length_change: f32,
    /// ... while it yields to its AI's walk and once handed back (degrees).
    pub yield_turn_jump: f32,
    pub react_v: f32,
    pub flinch_pelvis_drop: f32,
    pub step_pelvis_drop: f32,
    pub foot_drop: f32,
    pub under_floor: f32,
    pub reference_dz: f32,
    pub pelvis_jump: f32,
    pub bone_spin: f32,
    pub spin_bone: usize,
    /// When the physical body was last handed back to the animation (its weight in the shown pose
    /// down to nothing; the brace's arms may still be on their way home).
    pub handed_back_at: Option<f64>,
    /// Each hit's push into the active ragdoll (N s; Stopping Power's included).
    pub pushes: Vec<f32>,
    /// How many offsets `active::Limit` held over the run.
    pub limited: u32,
    pub yaw_rate: f32,
    pub yaw_off: f32,
}

/// The support hand's way back to its grip, watched ([`Sim::watch_regrip`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct RegripWatch {
    /// Seconds since it set off back (none: not watching); the way to the grip at the start.
    since: Option<f32>,
    toward: V3,
    /// The least distance to the grip so far (since the last swing), and whether it has got
    /// within the lock.
    least: f32,
    locked: bool,
    done: bool,
}

/// The support hand's distance to its grip growing again by more than this (units, 0.5 cm) is a
/// swing; within this of the grip (units) it is on it.
pub const REGRIP_SWING: f32 = 0.35;
pub const REGRIP_LOCK: f32 = 1.0;
/// How long the regrip is watched after the hand sets off back (s).
pub const REGRIP_WATCH: f32 = 3.0;

/// A bracing hand watched for bounce (the shown pose against the brace's surface).
#[derive(Clone, Copy, Debug, Default)]
pub struct HandWatch {
    touching: bool,
    since: f32,
    settled: Option<V3>,
    last: Option<V3>,
    elbow: Option<f32>,
    elbow_dir: f32,
    reversals: u32,
}

/// A hand touches when its wrist is within this of its rest on the surface (units).
pub const TOUCH: f32 = 0.75;
/// A touching hand counts as settled this long after it touched (s; its approach).
pub const SETTLE_TOUCH: f32 = hands::APPROACH_MAX + 0.02;
/// A planted elbow turning back by more than this (rad, 3 degrees) from the furthest it went is a
/// reversal (an oscillation); less is the torso's sway.
pub const ELBOW_SWING: f32 = 0.052;

/// An elbow's or a knee's bend in the pose `posed` (rad, negative: backwards past straight), from
/// the mannequin's own anatomy: about its parent's X axis, `sign` 1 for an elbow (the forearm
/// comes forward), -1 for a knee (the calf goes back); straight is the two bones in line (each
/// bone's own line at rest: joint to the child's joint).
pub fn hinge_bend(posed: &[Xf; COUNT], parent: usize, child: usize, sign: f32) -> f32 {
    let (Some(u), Some(c)) = (
        unit(sub(mq::end(parent), mq::JOINT[parent])),
        unit(sub(mq::end(child), mq::JOINT[child])),
    ) else {
        return 0.0;
    };
    let rel = mul(&transpose(&posed[parent].0), &posed[child].0);
    let f = apply_m(&rel, c);
    // In the hinge's plane (square to X): the turn from the parent's line to the child's.
    let (u2, f2) = ([0.0, u[1], u[2]], [0.0, f[1], f[2]]);
    sign * (u2[1] * f2[2] - u2[2] * f2[1]).atan2(dot(u2, f2))
}

/// How far the pose `posed` bends an elbow or a knee backwards past straight and the hinges'
/// allowance (`active::HYPEREXTEND`; degrees, 0 when none does), measured from the mannequin's
/// own anatomy ([`hinge_bend`]), not from the body's hinges; a joint folded round past its most
/// flex counts as flexed past its stop ([`past_stop`]), not as bent backwards.
pub fn back_bend(posed: &[Xf; COUNT]) -> f32 {
    let mut most: f32 = 0.0;
    for (parent, child, sign) in [
        (mq::L_UPPERARM, mq::L_FOREARM, 1.0f32),
        (mq::R_UPPERARM, mq::R_FOREARM, 1.0),
        (mq::L_THIGH, mq::L_CALF, -1.0),
        (mq::R_THIGH, mq::R_CALF, -1.0),
    ] {
        let flex = hinge_bend(posed, parent, child, sign);
        let over = past_stop(flex, hinge_flex(child));
        if over < 0.0 {
            most = most.max((-over).to_degrees());
        }
    }
    most
}

/// How far a hinge's bend `flex` (rad) is past its stops (straight, less `active::HYPEREXTEND`,
/// and `most` flexed), taking whichever end is nearer round the circle: negative backwards past
/// straight, positive flexed past its stop, 0 within.
pub fn past_stop(flex: f32, most: f32) -> f32 {
    if !flex.is_finite() {
        return 0.0;
    }
    let low = -active::HYPEREXTEND;
    if (low..=most).contains(&flex) {
        return 0.0;
    }
    let tau = std::f32::consts::TAU;
    let to_low = (low - flex).rem_euclid(tau);
    let to_high = (flex - most).rem_euclid(tau);
    if to_low <= to_high {
        -to_low
    } else {
        to_high
    }
}

/// The most an elbow (a forearm) or a knee (a calf) flexes: the lab ragdoll's stop.
fn hinge_flex(child: usize) -> f32 {
    let lim = mq::ragdoll_limits(child)[0];
    lim[1].max(-lim[0])
}

/// The lab's world as the reacting body asks it (`ragdoll_core::obstacle::World`; the host asks
/// the game's ray casts and navmesh).
struct Probe<'a>(&'a PhysicsWorld);

impl obstacle::World for Probe<'_> {
    fn floor(&mut self, at: V3) -> Option<V3> {
        world::ground_point(self.0, at)
    }
    fn cast(&mut self, from: V3, dir: V3, range: f32) -> Option<f32> {
        world::cast(self.0, from, dir, range)
    }
}

/// A Rapier vector as the lab's.
fn v3(v: Vec3) -> V3 {
    [v.x, v.y, v.z]
}

/// What a ray from a point along a direction hits on the body: the bone and the point.
pub type Pick = dyn Fn(&Sim, V3, V3) -> Option<(usize, V3)>;

/// The lab's world and its body.
pub struct Sim {
    pub world: PhysicsWorld,
    pub ground: Ground,
    pub tune: Tuning,
    pub body: Body,
    pub scale: f32,
    pub armour: f32,
    pub weapon: Weapon,
    pub range: f32,
    /// The animation's root: its heading (rad about Z), and how fast it turns (rad/s) and walks
    /// (units/s along +X) while it stands, as the game's AI moves an actor while it reacts.
    pub heading: f32,
    /// The heading it starts at (rad; `face=` degrees): 180 faces the obstacles (they stand at
    /// -Y), so a push from behind takes it into one in front of it.
    pub face: f32,
    pub turn: f32,
    pub walk: f32,
    /// The AI's move starting at this time (s; none: from the start: a scenario's `move` is after
    /// its last hit), and the
    /// turn stopping after this much of it (rad; none: never); how much it has turned.
    pub move_after: Option<f32>,
    pub turn_by: Option<f32>,
    pub turned: f32,
    /// The animation's gait while the AI moves it (its legs swing, the pelvis bobs, the spine
    /// turns into a turn): its phase (rad) and weight (eased in and out over [`GAIT_IN`]).
    pub gait_phase: f32,
    pub gait_w: f32,
    pub turning: f32,
    /// The host's read of the animation back from the bones it wrote (b44a964 and before:
    /// `pose::read_base` against the write left in them; the clip's stale base): each bone's
    /// remembered animated local and what was written over it. Off: the animation as it is (the
    /// host's write undone after the draw).
    pub read_back: bool,
    pub base: [Xf; COUNT],
    pub written: Option<[Xf; COUNT]>,
    pub base_ready: bool,
    /// With [`Sim::read_back`]: the write left in the bones, as b44a964 left it (its reads went
    /// stale under a walk: the clip's); off, the host puts the game's values back after the draw.
    pub leave_written: bool,
    /// `active::Limit` off (a test's comparison only).
    pub limit_off: bool,
    /// The animation's L Calf jumps from this time on (`Spec::leg_jump`); an animated jump to keep
    /// out of the body's carry this frame.
    pub leg_jump_at: Option<f64>,
    rebase_legs: bool,
    /// The player's perks of nv-rs's own (`ragdoll_core::perks`; the host's `perks`): every hit
    /// here is the player's.
    pub perks: Perks,
    /// With [`Sim::read_back`]: once the AI moves the actor, its animation leaves the upper spine
    /// to a later pass that turns it toward the attacker on top of what it holds (an aim, or a new
    /// set after a cripple: the clip's upper body spinning at the waist).
    pub aim_pass: bool,
    /// The steps' own moves of the actor this frame (the actor following the feet: its offset
    /// takes them back), and each bone's shown offset from its animation the frame before
    /// ([`Stats::pelvis_jump`], [`Stats::bone_spin`]).
    pub own_moved: V3,
    offsets_before: Option<[active::Q; COUNT]>,
    torso_before: Option<f32>,
    /// A still ragdoll's nudges to finish toppling (`knock::NUDGES`), and the last one's time.
    nudges: u32,
    nudge_at: f64,
    clean_before: Option<[Xf; COUNT]>,
    /// How much each shown foot is lifted (`active::lift_amount`).
    lift_low: [f32; 2],
    /// The steps' own moves of the actor not yet left out of the body's carry.
    pub own_carry: V3,
    was_holding: bool,
    /// The game's value of each bone as it was read (what the host puts back after the draw).
    pub raw: [Xf; COUNT],
    /// The body holds a weapon (aims it) and is attacking (an attack a big hit interrupts);
    /// the weapon is two-handed (the left hand supports it) or one-handed (a pistol).
    pub armed: bool,
    pub two_handed: bool,
    pub attacking: bool,
    /// The standing body's frame time (s): [`STEP`], or the game's range in the tests.
    pub dt: f32,
    /// The physical body's bone parents ([`mq::PARENT`], the game's node tree's physics parents);
    /// a test sets the node tree itself to see what the game did before (the spine a free root).
    pub body_parents: [Option<usize>; COUNT],
    pub t: f64,
    pub pos: V3,
    pub rig: Rig,
    pub state: State,
    pub stack: Option<Stack>,
    /// What hits have left in the body: fatigue, pain, injured limbs (`ragdoll_core::condition`;
    /// the host keeps one per actor).
    pub condition: Condition,
    pub last_knock: Option<f64>,
    pub last_up: Option<f64>,
    pub posed: [Xf; COUNT],
    /// The pose a frame before (the body's starting motion; the smoothness measures).
    pub posed_before: [Xf; COUNT],
    /// The brace's hands watched for bounce ([`HandWatch`]); a planted hand's shown rotation, held
    /// as it touched.
    pub watch: [HandWatch; 2],
    pub hand_lock: [Option<M3>; 2],
    /// Each free arm's wrist this frame (before the brace's IK).
    pub free_wrist: [Option<V3>; 2],
    /// The support hand's regrip, watched; the grip hold on ([`Sim::grip_hold_weight`]).
    pub regrip: RegripWatch,
    pub grip_hold: bool,
    velocity: Option<[V3; COUNT]>,
    /// The animated pelvis and the shown pelvis's turn against it the frame before ([`Stats::yaw_rate`]).
    anim_pelvis: Option<(V3, f32)>,
    /// The pelvis's offset when nothing held the body any more ([`Stats::release_rise`]).
    release_off: Option<f32>,
    acceleration: Option<[V3; COUNT]>,
    pub stats: Stats,
    pub log: Vec<String>,
    rng: u64,
}

const IDENT_XF: Xf = (IDENTITY, [0.0; 3]);

impl Sim {
    pub fn new(kind: GroundKind) -> Sim {
        let mut world = PhysicsWorld::new();
        world.gravity = Vec3::new(0.0, 0.0, -9.81);
        world.integration_parameters.dt = STEP;
        let ground = world::build(&mut world, kind);
        let mut sim = Sim {
            world,
            ground,
            tune: tuning::get(),
            body: Body::Human,
            scale: 1.0,
            armour: 0.0,
            weapon: Weapon::default(),
            range: 10.0,
            heading: 0.0,
            face: 0.0,
            turn: 0.0,
            walk: 0.0,
            move_after: None,
            turn_by: None,
            turned: 0.0,
            gait_phase: 0.0,
            gait_w: 0.0,
            turning: 0.0,
            read_back: false,
            base: [IDENT_XF; COUNT],
            written: None,
            base_ready: false,
            leave_written: false,
            limit_off: false,
            leg_jump_at: None,
            rebase_legs: false,
            perks: Perks::default(),
            aim_pass: false,
            own_moved: [0.0; 3],
            offsets_before: None,
            torso_before: None,
            nudges: 0,
            nudge_at: 0.0,
            clean_before: None,
            lift_low: [0.0; 2],
            own_carry: [0.0; 3],
            was_holding: false,
            raw: [IDENT_XF; COUNT],
            armed: false,
            two_handed: true,
            attacking: false,
            dt: STEP,
            body_parents: mq::PARENT,
            t: 0.0,
            pos: [0.0; 3],
            rig: Rig::new(Limbs::WHOLE),
            state: State::Standing,
            stack: None,
            condition: Condition::default(),
            last_knock: None,
            last_up: None,
            posed: [IDENT_XF; COUNT],
            posed_before: [IDENT_XF; COUNT],
            watch: [HandWatch::default(); 2],
            hand_lock: [None; 2],
            free_wrist: [None; 2],
            regrip: RegripWatch::default(),
            grip_hold: false,
            velocity: None,
            anim_pelvis: None,
            release_off: None,
            acceleration: None,
            stats: Stats::default(),
            log: Vec::new(),
            rng: 0x2545_f491_4f6c_dd1d,
        };
        sim.reset();
        sim
    }

    /// The lab's tuning made the one in force (`ragdoll_core::tuning::set`).
    pub fn set_tuning(&mut self, t: Tuning) {
        self.tune = t;
        tuning::set(t);
    }

    /// A new ground: the body back at its start on it.
    pub fn set_ground(&mut self, kind: GroundKind) {
        let mut world = PhysicsWorld::new();
        world.gravity = Vec3::new(0.0, 0.0, -9.81);
        world.integration_parameters.dt = STEP;
        self.ground = world::build(&mut world, kind);
        self.world = world;
        self.state = State::Standing;
        self.reset();
    }

    /// The body stood up at the start, everything forgotten.
    pub fn reset(&mut self) {
        tuning::set(self.tune);
        if let State::Ragdoll { bodies, .. } = &self.state {
            for h in bodies.iter().flatten() {
                self.world.remove_body(*h);
            }
        }
        let start = world::ground_point(&self.world, [0.0, 0.0, 0.0]).unwrap_or([0.0; 3]);
        self.pos = start;
        self.heading = self.face;
        self.turned = 0.0;
        self.gait_phase = 0.0;
        self.gait_w = 0.0;
        self.turning = 0.0;
        self.base = [IDENT_XF; COUNT];
        self.written = None;
        self.base_ready = false;
        self.rig = Rig::new(self.rig.limbs);
        self.state = State::Standing;
        self.stack = None;
        // A fresh body: no fatigue, pain or hurt limbs left from an earlier run.
        self.condition = Condition::default();
        self.watch = [HandWatch::default(); 2];
        self.regrip = RegripWatch::default();
        self.grip_hold = false;
        self.last_knock = None;
        self.last_up = None;
        self.stats = Stats {
            start,
            ..Stats::default()
        };
        self.t = 0.0;
        self.posed = self.anim();
        self.posed_before = self.posed;
        self.velocity = None;
        self.anim_pelvis = None;
        self.offsets_before = None;
        self.torso_before = None;
        self.clean_before = None;
        self.lift_low = [0.0; 2];
        self.own_moved = [0.0; 3];
        self.acceleration = None;
        self.note("reset: standing at the start".into());
    }

    pub fn set_limbs(&mut self, limbs: Limbs) {
        self.rig.limbs = limbs;
    }

    pub fn note(&mut self, line: String) {
        self.log.push(format!("{:6.2} s  {line}", self.t));
        if self.log.len() > 400 {
            self.log.drain(..100);
        }
    }

    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn mass(&self) -> f32 {
        body_mass(self.body, self.scale)
    }

    pub fn topple(&self) -> f32 {
        topple_speed(
            self.body,
            self.scale,
            self.tune.knockdown_threshold.round() as i32,
        )
    }

    /// The animated pose: the rest pose at the reference (identity rotations).
    pub fn anim(&self) -> [Xf; COUNT] {
        if self.read_back && self.base_ready {
            return self.fk(&self.base);
        }
        let locals: [Xf; COUNT] = std::array::from_fn(|b| self.true_local(b));
        self.fk(&locals)
    }

    /// The pose from bone locals, down from the root (its heading and position).
    fn fk(&self, locals: &[Xf; COUNT]) -> [Xf; COUNT] {
        let mut out = [IDENT_XF; COUNT];
        for b in 0..COUNT {
            let (prot, ppos) = match mq::PARENT[b] {
                Some(p) => out[p],
                None => (rotation([0.0, 0.0, self.heading]), self.pos),
            };
            out[b] = (
                mul(&prot, &locals[b].0),
                add(ppos, apply_m(&prot, locals[b].1)),
            );
        }
        out
    }

    /// A bone's local as the game's animation writes it this frame: its pose ([`Sim::anim_local`])
    /// and the gait while the AI moves the actor ([`Sim::gait_w`]).
    pub fn true_local(&self, b: usize) -> Xf {
        self.local_of(b, true)
    }

    /// The animation as it would be without the forced leg jump (`Spec::leg_jump`): its own
    /// motion, which the drawn pose may follow ([`Stats::drawn_move`]).
    fn clean_anim(&self) -> [Xf; COUNT] {
        let locals: [Xf; COUNT] = std::array::from_fn(|b| self.local_of(b, false));
        self.fk(&locals)
    }

    /// [`Sim::true_local`], with the forced leg jump or without.
    fn local_of(&self, b: usize, jump: bool) -> Xf {
        let (w, ph) = (self.gait_w, self.gait_phase);
        let mut r = self.anim_local(b);
        let mut t = self.rest_local(b);
        if jump && b == mq::L_CALF && self.leg_jump_at.is_some_and(|at| self.t >= at - 1e-6) {
            r = mul(&r, &rotation([0.3, 0.0, 0.0]));
        }
        if w > 0.0 {
            match b {
                mq::L_THIGH => r = mul(&r, &rotation([0.45 * w * ph.sin(), 0.0, 0.0])),
                mq::R_THIGH => r = mul(&r, &rotation([-0.45 * w * ph.sin(), 0.0, 0.0])),
                mq::SPINE1 => r = mul(&r, &rotation([0.0, 0.0, 0.25 * self.turning])),
                mq::PELVIS => t[2] -= 1.5 * w * (1.0 - (2.0 * ph).cos()) * 0.5,
                // The feet lift and swing (the walk's foot locals move: the clip's L Foot).
                mq::L_FOOT => {
                    t[2] += 6.0 * w * ph.sin().max(0.0);
                    t[1] += 4.0 * w * ph.sin();
                }
                mq::R_FOOT => {
                    t[2] += 6.0 * w * (-ph.sin()).max(0.0);
                    t[1] -= 4.0 * w * ph.sin();
                }
                _ => {}
            }
        }
        (r, t)
    }

    /// Whether the AI walks or turns the actor now (`ragdoll_core::active::ai_walking`).
    fn ai_walking(&self) -> bool {
        let turning = self.turn_by.is_none_or(|by| self.turned < by);
        self.standing()
            && self.moving()
            && active::ai_walking(self.walk.abs(), if turning { self.turn } else { 0.0 })
    }

    /// Whether the AI moves the actor now ([`Sim::move_after`]).
    fn moving(&self) -> bool {
        match self.move_after {
            None => true,
            Some(a) => self.t >= a as f64,
        }
    }

    /// The host's read of the animation back from the bones (b44a964: [`Sim::read_back`]): the
    /// game's animation writes each bone's local; what it holds is read against what was written
    /// over it (`pose::read_base`), an animated jump eased in while it reacts (`pose::capped`).
    fn read_bases(&mut self, dt: f32) {
        let reacting = self.rig.body.is_some() || self.rig.fade.is_some();
        let k = (dt * 60.0).clamp(0.5, 3.0);
        let first = !self.base_ready;
        self.base_ready = true;
        let mut jumped = false;
        let mut jumped_roles: Vec<Role> = Vec::new();
        let mut leg_jump = false;
        let moving = self.moving() && self.gait_w > 0.0;
        for b in 0..COUNT {
            let mut now = self.true_local(b);
            if self.aim_pass && moving && matches!(b, mq::SPINE1 | mq::SPINE2) {
                // Not the animation's: what it holds, turned by the aim toward the attacker (back
                // the way the actor turned, at most 0.6 rad a bone).
                let held = self.written.map_or(self.raw[b], |w| w[b]);
                let yaw = active::heading_off(&held.0, FORWARD);
                let want = (-self.heading * 0.5).clamp(-0.6, 0.6);
                now = (
                    mul(&held.0, &rotation([0.0, 0.0, 0.3 * (want - yaw)])),
                    held.1,
                );
            }
            if first {
                self.base[b] = now;
                continue;
            }
            let (mut base, how) =
                ragdoll_core::pose::read_base(now, Some(self.base[b]), self.written.map(|w| w[b]));
            if how == ragdoll_core::pose::Read::Animated && reacting {
                if mq::role_side(b).0.is_leg() && active::leg_jumped(&self.base[b], &base) {
                    leg_jump = true;
                }
                let (eased, cut) = ragdoll_core::pose::capped(&self.base[b], &base, k);
                if cut {
                    self.stats.base_jumps += 1;
                    // b44a964 eased it in; now the reaction yields to it.
                    if self.leave_written {
                        base = eased;
                    } else {
                        jumped = true;
                        jumped_roles.push(mq::role_side(b).0);
                    }
                }
            }
            if how == ragdoll_core::pose::Read::Disturbed {
                self.stats.base_stale += 1;
            }
            self.base[b] = base;
            self.raw[b] = now;
        }
        // A leg-only jump right after a hit is re-based (the host's too), else yielded to.
        let since_hit = self
            .stats
            .last_hit
            .map_or(f32::MAX, |h| (self.t - h) as f32);
        // Any jump is kept out of the body's carry (the host's too).
        self.rebase_legs = self.rig.body.is_some() && (jumped || leg_jump);
        if self.rig.body.is_some() && active::rebases(since_hit, leg_jump, &jumped_roles) {
            self.stats.rebased += 1;
            self.note(format!(
                "its animation jumped in the legs {since_hit:.2} s after the hit: re-based"
            ));
        } else if jumped {
            self.yield_now("its animation jumped");
        }
    }

    /// The reaction yields to its animation now (`ragdoll_core::active::YIELD_BACK`): its body
    /// handed back quickly, a brace's hands let go, the steps end. Once a hit.
    fn yield_now(&mut self, why: &str) {
        if self.rig.yield_at.is_some()
            || self.rig.body.is_none()
            || self.rig.fall.is_some()
            || self.stats.last_hit.is_none()
        {
            return;
        }
        self.rig.yield_at = Some(self.t);
        if let Some(s) = self.rig.balance.as_mut() {
            s.recovering = true;
        }
        self.note(format!(
            "yields to its animation ({why}): handed back over {} s",
            active::YIELD_BACK
        ));
    }

    /// What the host wrote over the bones this frame: nothing left in them (it puts the game's own
    /// values back after the draw), or with [`Sim::leave_written`] (b44a964) the shown pose's
    /// locals while it reacts.
    fn write_back(&mut self) {
        if !(self.rig.body.is_some() || self.rig.fade.is_some()) {
            self.written = None;
            return;
        }
        if !self.leave_written {
            self.written = None;
            return;
        }
        let root = (rotation([0.0, 0.0, self.heading]), self.pos);
        let posed = self.posed;
        self.written = Some(std::array::from_fn(|b| {
            let (pr, pp) = mq::PARENT[b].map_or(root, |p| posed[p]);
            let it = transpose(&pr);
            (mul(&it, &posed[b].0), apply_m(&it, sub(posed[b].1, pp)))
        }));
    }

    /// A bone's animated local rotation: the rest pose, or (armed) the aim: the right arm out
    /// along the view, the left supporting.
    pub fn anim_local(&self, b: usize) -> M3 {
        if !self.armed {
            return IDENTITY;
        }
        match b {
            mq::R_UPPERARM => rotation([1.3, 0.0, 0.25]),
            mq::R_FOREARM => rotation([0.3, 0.0, 0.0]),
            // A one-handed weapon: the left arm hangs.
            mq::L_UPPERARM | mq::L_FOREARM if !self.two_handed => IDENTITY,
            mq::L_UPPERARM => rotation([1.1, 0.0, -0.55]),
            mq::L_FOREARM => rotation([0.75, 0.0, 0.0]),
            mq::SPINE2 => rotation([0.0, 0.0, -0.12]),
            _ => IDENTITY,
        }
    }

    /// An attack interrupted by a big enough hit (the host's `active_ragdoll::interrupt_attack`).
    fn interrupt_attack(&mut self, why: &str) {
        if self.attacking {
            self.attacking = false;
            self.stats.interrupted = true;
            self.note(format!("attack interrupted ({why})"));
        }
    }

    /// The rest local translation of a bone (from its parent's joint, or the reference).
    fn rest_local(&self, b: usize) -> V3 {
        let parent = mq::PARENT[b].map_or([0.0; 3], |p| mq::JOINT[p]);
        scale(sub(mq::JOINT[b], parent), self.scale)
    }

    pub fn state_is_standing(&self) -> bool {
        matches!(self.state, State::Standing)
    }

    pub fn standing(&self) -> bool {
        matches!(self.state, State::Standing)
    }

    pub fn in_ragdoll(&self) -> bool {
        matches!(self.state, State::Ragdoll { .. })
    }

    // --- Hits (the host's `knockdown::after_hit`). -----------------------------------------------

    /// One hit: `push` at `point` along `direction` (unit).
    pub fn hit(&mut self, push: Push, point: V3, direction: V3, part: &str) {
        tuning::set(self.tune);
        self.stats.hits += 1;
        self.stats.first_hit.get_or_insert(self.t);
        self.stats.last_hit = Some(self.t);
        self.stats.recovered_at = None;
        let dt = match push {
            Push::Blast { .. } => 0.0,
            _ => self.armour,
        };
        let p = transferred(push, dt);
        match self.state {
            State::Ragdoll { ref bodies, .. } => {
                // In its ragdoll: the struck bone takes the hit's momentum (the lab's; see the
                // module documentation).
                let bone = self.nearest_bone(point);
                if let Some(Some(h)) = bodies.get(bone) {
                    let k = self.tune.hit_force.max(0.0) / 100.0;
                    if let Some(b) = self.world.bodies.get_mut(*h) {
                        b.apply_impulse_at_point(vec(scale(direction, p * k)), to_m(point), true);
                    }
                }
                self.note(format!(
                    "{part}: {p:.1} N s into its ragdoll ({})",
                    mq::short_name(bone)
                ));
                return;
            }
            State::GettingUp { .. } => {
                self.note(format!(
                    "{part}: not knocked (knock state: getting up), {p:.1} N s"
                ));
                return;
            }
            State::Standing => {}
        }
        if self.tune.knockdowns < 0.5 {
            self.note(format!(
                "{part}: {p:.1} N s; bKnockdowns=0: no reaction (the game's after_hit returns)"
            ));
            return;
        }
        if self.rig.fall.is_some() {
            self.note("going over into its ragdoll: this hit waits for it".into());
            return;
        }
        let half_life = self.tune.impact_stacking.max(1.0) / 1000.0;
        let old = self.stack.unwrap_or_default();
        let seconds = (self.t - old.at) as f32;
        let (stacked, vector) = add_hit(
            old.total, old.vector, old.hits, seconds, half_life, p, direction,
        );
        self.stack = Some(Stack {
            total: stacked.total,
            vector,
            hits: stacked.hits,
            at: self.t,
        });
        let mass = self.mass();
        let hit_force = self.tune.hit_force.max(0.0) / 100.0;
        let speed = stacked.total / mass * hit_force;
        let topple = self.topple();
        // Pain (this hit's own push over the topple speed, added to what is left of earlier
        // hits') escalates the reaction: sustained fire staggers, then knocks down.
        let (role, side) = mq::role_side(self.nearest_bone(point));
        let own = p / mass * hit_force;
        self.condition
            .hit(own / topple.max(0.1), condition::region_of(role, side));
        // Stopping Power: the stagger (an attack interrupted) from a lower share of the topple speed,
        // and the push into the active ragdoll harder; the stack and the knock-down are the hit's.
        let stopping = self.perks.stopping_power;
        let decision = condition::escalate(
            perks::reaction(speed, topple, stopping),
            self.condition.stage(),
        );
        let pushed = perks::stopping_push(p, stopping);
        self.stats.pushes.push(pushed);
        self.stats.peak_speed = self.stats.peak_speed.max(speed);
        self.stats.topple = topple;
        self.stats.decisions.push(format!("{decision:?}"));
        self.note(format!(
            "{part}: {} {p:.1} N s (DT {dt:.0}); stack {:.1} + this = {:.1} N s over {} hits \
             (half-life {half_life:.2} s) -> {speed:.2} m/s on {mass:.0} kg vs topple {topple:.2} \
             m/s -> {decision:?}",
            describe(push),
            stacked.before,
            stacked.total,
            stacked.hits
        ));
        match decision {
            Reaction::Knockdown => {
                let max = self.tune.max_launch;
                if speed > max {
                    self.note(format!("clamp: launch {speed:.1} m/s held to {max} m/s"));
                }
                self.stack = None;
                self.flinch(pushed, mass, point, direction, false);
                // Not knocked down now (just knocked down, just got up): no fall pose, a stagger
                // (the host's `knockdown::can_knock_now`).
                if let Err(why) = self.can_knock_now() {
                    self.note(format!("no fall pose: {why}: a stagger"));
                    self.interrupt_attack("staggered");
                    self.push(pushed, mass, direction);
                    return;
                }
                self.interrupt_attack("knocked down");
                if !self.fall(point, stacked.direction, speed) {
                    self.knock_down(stacked.direction, speed, "knockdown");
                }
            }
            Reaction::Stagger => {
                self.flinch(pushed, mass, point, direction, false);
                self.interrupt_attack("staggered");
                self.push(pushed, mass, direction);
            }
            Reaction::None => {
                self.flinch(pushed, mass, point, direction, true);
                self.push(pushed, mass, direction);
            }
        }
    }

    /// Bowling Ball (the host's `perks::bowling`): a body the player knocked over, of `mass` (kg),
    /// flying into this actor at `speed` (m/s) along `direction`: a push of its momentum's share
    /// (`ragdoll_core::perks::bowling_push`), as a hit's at the pelvis, no damage. Answers the
    /// reaction (none without the perk).
    pub fn bump(&mut self, mass: f32, speed: f32, direction: V3) -> Option<Reaction> {
        if !self.perks.bowling_ball || !self.standing() {
            return None;
        }
        let p = perks::bowling_push(mass, speed);
        if p <= 0.0 {
            return None;
        }
        let own = self.mass();
        let v = p / own;
        let topple = self.topple();
        let point = self.posed[mq::PELVIS].1;
        let decision = reaction(v, topple);
        self.stats.hits += 1;
        self.stats.first_hit.get_or_insert(self.t);
        self.stats.last_hit = Some(self.t);
        self.stats.peak_speed = self.stats.peak_speed.max(v);
        self.stats.topple = topple;
        self.stats.decisions.push(format!("{decision:?}"));
        self.stats.pushes.push(p);
        self.note(format!(
            "bowling ball: a body of {mass:.0} kg at {speed:.1} m/s: {p:.0} N s -> {v:.2} m/s on \
             {own:.0} kg vs topple {topple:.2} m/s -> {decision:?}"
        ));
        match decision {
            Reaction::Knockdown => {
                self.flinch(p, own, point, direction, false);
                self.interrupt_attack("knocked down");
                if !self.fall(point, direction, v) {
                    self.knock_down(direction, v, "bowling ball");
                }
            }
            Reaction::Stagger => {
                self.flinch(p, own, point, direction, false);
                self.interrupt_attack("staggered");
                self.push(p, own, direction);
            }
            Reaction::None => {
                self.flinch(p, own, point, direction, true);
                self.push(p, own, direction);
            }
        }
        Some(decision)
    }

    /// The struck bone (the host's): the one whose segment the impact is on
    /// (`ragdoll_core::flinch::struck_bone`).
    fn nearest_bone(&self, point: V3) -> usize {
        let joints: Vec<V3> = self.posed.iter().map(|x| x.1).collect();
        ragdoll_core::flinch::struck_bone(&joints, &mq::PARENT, point)
    }

    // --- Stage 1 (the host's `active_ragdoll::hit`). ---------------------------------------------

    fn flinch(&mut self, p: f32, mass: f32, point: V3, direction: V3, keep_aim: bool) {
        let strength = self.tune.hit_reactions;
        if strength <= 0.0 {
            self.note("no flinch: Hit Reactions is 0".into());
            return;
        }
        if !(p > 0.0 && mass > 0.0) {
            self.note(format!("no flinch: no momentum ({p:.2} N s)"));
            return;
        }
        let Some(direction) = unit(direction) else {
            self.note("no flinch: the hit has no direction".into());
            return;
        };
        let t = self.tune;
        let v = p / mass * strength / 100.0;
        // A hit while it is handed back: a fresh body from the pose shown (its weight jumped back
        // to full over a body left behind: the right hand 43 rad/s in one frame).
        if self.rig.body.is_some() && self.rig.shown_w < 0.999 {
            self.rig.body = None;
        }
        self.stats.react_v = self.stats.react_v.max(v);
        self.rig.react_v = if self.rig.body.is_some() {
            self.rig.react_v.max(v)
        } else {
            v
        };
        self.rig.yield_at = None;
        self.rig.moved_since_hit = 0.0;
        self.rig.turned_since_hit = 0.0;
        self.rig.since = self.t;
        self.rig.active = true;
        self.rig.aim_hold = if self.armed && keep_aim {
            t.aim_hold
        } else {
            0.0
        };
        let spread = spread_of(v);
        let struck = self.nearest_bone(point);
        // The muscles near the hit go slack for a moment (the spread); an armed body's arms hold
        // their aim (stronger) through a hit below the stagger.
        let positions: Vec<V3> = self.posed.iter().map(|x| x.1).collect();
        let dips: Vec<f32> = (0..COUNT)
            .map(|b| {
                let role = mq::role_side(b).0;
                let d = length(sub(positions[b], point)) / UNITS_PER_METRE;
                let dip = t.muscle_dip * weight(role, d, spread).min(1.0);
                if role.is_arm() && self.rig.aim_hold > 0.0 {
                    dip - 5.0 * self.rig.aim_hold
                } else {
                    dip
                }
            })
            .collect();
        let touched = dips.iter().filter(|d| **d > 0.02).count();
        self.ensure_body();
        self.rig.hit_at = self.t;
        self.release_off = None;
        self.stats.handed_back_at = None;
        self.rig.since = self.t;
        let m = 1.0 / UNITS_PER_METRE;
        if let Some(body) = self.rig.body.as_mut() {
            let takes: Vec<f32> = (0..COUNT)
                .map(|b| {
                    if self.rig.aim_hold > 0.0 && mq::role_side(b).0.is_arm() {
                        0.0
                    } else {
                        1.0
                    }
                })
                .collect();
            body.hit(
                p * strength / 100.0,
                direction,
                scale(sub(point, self.rig.origin), m),
                spread,
                &dips,
                &takes,
            );
        }
        // The flinch drawn on the torso (the host's too).
        if self.rig.body.is_some() {
            let role = mq::role_side(struck).0;
            let head = matches!(role, Role::Head | Role::Neck);
            let chest = self.posed[mq::SPINE2].1;
            let facing = self.anim_facing();
            self.rig.flinches.push(active::TorsoFlinch::new(
                v,
                direction,
                point,
                chest,
                facing,
                head,
                role.is_arm().then_some(mq::role_side(struck).1),
            ));
        }
        self.stats.flinches += 1;
        self.note(format!(
            "flinch at {}: {p:.1} N s on {mass:.0} kg -> {v:.2} m/s (Hit Reactions {strength:.0}%), \
             spread {spread:.2} m, {touched} of {COUNT} bones{}",
            BONES[struck].0,
            match (self.armed, self.rig.aim_hold > 0.0, self.attacking) {
                (true, true, true) => "; armed and attacking: keeps its aim",
                (true, true, false) => "; armed: keeps its aim",
                (true, false, _) => "; armed: the hit takes its aim",
                (false, _, true) => "; attacking",
                _ => "",
            }
        ));
    }

    // --- Stage 2 (the host's `active_ragdoll::push`). --------------------------------------------

    fn push(&mut self, p: f32, mass: f32, direction: V3) {
        let strength = self.tune.balance;
        if strength <= 0.0 || !(p > 0.0 && mass > 0.0) {
            return;
        }
        let Some(dir) = unit([direction[0], direction[1], 0.0]) else {
            return;
        };
        let v = p / mass * strength / 100.0;
        // The AI walking or turning it: its stride carries it, no catch steps.
        if self.ai_walking() {
            self.note("no catch steps: its AI is walking it".into());
            return;
        }
        let scale_ = self.scale;
        let one_leg = !self.rig.limbs.legs();
        let anim = self.anim();
        let pelvis = add(anim[mq::PELVIS].1, self.rig.shift);
        let (feet, carried) = match &self.rig.balance {
            Some(s) if !s.recovering => (s.planted, s.v * 0.5),
            _ => ([self.posed[mq::L_FOOT].1, self.posed[mq::R_FOOT].1], 0.0),
        };
        let v = v + carried;
        let h = ((pelvis[2] - (feet[0][2] + feet[1][2]) / 2.0) / UNITS_PER_METRE).clamp(0.3, 2.5);
        let planned = plan(pelvis, feet, dir, v, h, scale_);
        if planned.steps.is_empty() {
            return;
        }
        self.rig.active = true;
        // Pushed past its feet: no aim kept, an attack interrupted.
        self.rig.aim_hold = 0.0;
        self.interrupt_attack(if one_leg {
            "it falls"
        } else {
            "pushed past its feet"
        });
        if one_leg {
            self.rig.fall = Some(Fall {
                dir,
                speed: v,
                point: pelvis,
                t: 0.0,
                lead: self.tune.fall_lead,
                windmill: false,
                why: "one leg",
                fold: None,
            });
            self.rig.balance = None;
            self.note(format!("on one leg it cannot step: falls ({v:.2} m/s)"));
            return;
        }
        let outcome =
            if planned.need > self.tune.step_length * scale_ * self.tune.max_steps() as f32 {
                Outcome::Overrun
            } else {
                Outcome::Caught
            };
        let start = feet;
        let ankle: [f32; 2] = [0, 1].map(|f| {
            world::ground_point(&self.world, start[f])
                .map(|g| start[f][2] - g[2])
                .filter(|h| (0.0..=25.0).contains(h))
                .unwrap_or(8.0 * scale_)
        });
        // The steps on the world (`ragdoll_core::obstacle::resolve`, the host's too): floors,
        // walls, ledges, and low obstacles a swinging foot catches on.
        // What the body meets along the push (a railing's gaps let a foot's knee-height ray
        // through, not the body): no step lands past it.
        let floor_z = world::ground_point(&self.world, pelvis).map_or(0.0, |g| g[2]);
        let ahead = obstacle::scan(
            &mut Probe(&self.world),
            pelvis,
            floor_z,
            dir,
            planned.need + BRACE_PROBE,
            scale_,
        )
        .map(|o| (pelvis, dir, o));
        let r = obstacle::resolve(
            &mut Probe(&self.world),
            &planned.steps,
            feet,
            ankle,
            scale_,
            outcome,
            ahead,
        );
        let (mut steps, mut outcome, mut said) = (r.steps, r.outcome, r.said);
        if let (Outcome::Trip, Some(o)) = (outcome, r.tripped) {
            self.stats.tripped = true;
            if obstacle::trips_over(v, self.condition.leg_weakness()) {
                self.rig.fall = Some(Fall {
                    dir,
                    speed: v,
                    point: pelvis,
                    t: 0.0,
                    lead: self.tune.fall_lead,
                    windmill: false,
                    why: "tripped over a low obstacle",
                    fold: None,
                });
                self.rig.balance = None;
                self.note(format!(
                    "pushed {v:.2} m/s: its foot caught on a {:.0}-unit {}, it pitches over it",
                    o.height,
                    o.kind.name()
                ));
                return;
            }
            // Little push left: it stumbles, then steps over it.
            if let Some(&(foot, caught)) = steps.last() {
                if let Some(to) = obstacle::recovery_step(
                    &mut Probe(&self.world),
                    caught,
                    &o,
                    dir,
                    ankle[foot],
                    scale_,
                ) {
                    steps.push((foot, to));
                }
            }
            outcome = Outcome::Caught;
            said.push("it stumbles, then steps over it");
        }
        let against_wall = outcome == Outcome::Wall;
        if against_wall {
            self.start_brace(dir, v);
        }
        if steps.is_empty() {
            self.note(format!(
                "pushed {v:.2} m/s past its feet ({:.2} m): against a wall, it catches itself with \
                 its hands",
                planned.need
            ));
            return;
        }
        let count = steps.len();
        let lengths: Vec<String> = steps
            .iter()
            .map(|(f, to)| {
                format!(
                    "{:.2}",
                    length(sub([to[0], to[1], 0.0], [start[*f][0], start[*f][1], 0.0]))
                        / UNITS_PER_METRE
                )
            })
            .collect();
        let stepping = Stepping::new(dir, v, 0, start, steps, scale_, self.pos, outcome, self.t);
        self.rig.balance = Some(stepping);
        if against_wall && self.rig.brace.is_none() {
            let (wall, top, _) = self.brace_obstacle(dir);
            self.rig.brace = Some(self.new_brace(dir, v, wall, top));
        }
        self.rig.since = self.t;
        self.note(format!(
            "balance: pushed {v:.2} m/s along ({:.2}, {:.2}), capture point {:.2} m past its feet: \
             {count} step(s) ({} m){}; {outcome:?}",
            dir[0],
            dir[1],
            planned.need,
            lengths.join(", "),
            if said.is_empty() {
                String::new()
            } else {
                format!(", {}", said.join(", "))
            },
        ));
    }

    // --- Stage 3 (the host's `start_brace`, `reach_arm`, `brace_frame`, `fall_frame`, `fall`). ---

    /// The wall's distance ahead of the shoulders along `dir` (units), by a ray cast (the host's,
    /// by the game's).
    fn wall_ahead(&self, dir: V3) -> Option<f32> {
        let anim = self.anim();
        let u = mq::bone_of(Role::UpperArm, 0)?;
        let v = mq::bone_of(Role::UpperArm, 1).unwrap_or(u);
        let at = scale(add(anim[u].1, anim[v].1), 0.5);
        world::wall_ahead(&self.world, at, dir, BRACE_PROBE)
    }

    /// What the brace meets along `dir` (the host's `brace_obstacle`): the obstacle's distance,
    /// its top when it is waist-high (the hands go on it), and its kind; rays at several heights
    /// and across the body (`ragdoll_core::obstacle::scan`), else the shoulders' ray.
    fn brace_obstacle(&self, dir: V3) -> (Option<f32>, Option<f32>, Option<obstacle::Kind>) {
        let anim = self.anim();
        let at = anim[mq::PELVIS].1;
        let floor_z = world::ground_point(&self.world, at).map_or(0.0, |g| g[2]);
        match obstacle::scan(
            &mut Probe(&self.world),
            at,
            floor_z,
            dir,
            BRACE_PROBE,
            self.scale,
        ) {
            Some(o) => (
                Some(o.distance),
                (o.kind == obstacle::Kind::Waist).then_some(o.top),
                Some(o.kind),
            ),
            None => (self.wall_ahead(dir), None, None),
        }
    }

    fn start_brace(&mut self, dir: V3, v: f32) {
        let limit = self.tune.brace_holds * self.tune.balance.max(1.0) / 100.0;
        let arms = self.rig.limbs.arm(0) || self.rig.limbs.arm(1);
        let (wall, top, kind) = self.brace_obstacle(dir);
        // An obstacle behind the torso: no brace is begun (nor said), unless the push is too much
        // and the body goes over it (the host's too).
        let anim = self.anim();
        let behind = Surface::new(anim[mq::PELVIS].1, dir, wall, top)
            .is_some_and(|s| hands::behind(anim[mq::PELVIS].1, self.anim_facing(), s.face));
        if behind && v <= limit {
            return;
        }
        if arms && !behind {
            // The brace is the reaction now: the drawn flinch (begun with this hit) goes (the
            // host's too; drawn on, it swung a rifle's grip from under the regripping hand).
            self.rig.flinches.clear();
            // Its torso drawn as the body has it at once (the body has barely moved yet).
            self.rig.torso_keep = 1.0;
            self.rig.brace = Some(self.new_brace(dir, v, wall, top));
            self.stats.braced = true;
            self.stats.brace_kind = Some(if top.is_some() { "waist" } else { "wall" }.to_string());
        }
        if v > limit || !arms {
            let point = self.anim()[mq::PELVIS].1;
            // Over a waist-high obstacle the body folds over it (its hips pivot on the top); a
            // wall it crumples against.
            let fold = top.filter(|_| kind == Some(obstacle::Kind::Waist));
            self.rig.fall = Some(Fall {
                dir,
                speed: if fold.is_some() { v } else { v * 0.5 },
                point,
                t: 0.0,
                lead: self.tune.brace_reach + 0.1,
                windmill: false,
                why: if fold.is_some() {
                    "folded over a waist-high obstacle"
                } else {
                    "crumpled against a wall"
                },
                fold,
            });
        }
        let which = self
            .rig
            .brace
            .map_or([false; 2], |b| b.hands.map(|h| h.is_some()));
        self.note(format!(
            "braces against a wall with {}{}",
            match (arms, which) {
                (false, _) => "no arm to brace with",
                (true, [true, false]) if self.armed =>
                    "its free (left) hand, the weapon kept on its aim",
                (true, [true, true]) if self.armed => "both hands, the weapon kept in hand",
                (true, [true, false]) => "its left hand",
                (true, [false, true]) => "its right hand",
                _ => "its hands",
            },
            if v > limit || !arms {
                format!(": the push ({v:.2} m/s) is too much, it crumples into its ragdoll")
            } else {
                String::new()
            }
        ));
    }

    /// The ragdoll's elbows and knees held to their stops, as the standing body's are
    /// (`active::Body::stop_hinges`): past one (Rapier's limits give under a hard fall, a sledge's
    /// spin), the spin of the child against its parent about the hinge (its parent's X axis) that
    /// would take it further is taken out, shared by the two bodies, and it is turned back a share
    /// of the way each step ([`past_stop`]: whichever stop is nearer round the circle).
    fn stop_ragdoll_hinges(&mut self, bodies: &[Option<RigidBodyHandle>]) {
        for (parent, child, sign) in [
            (mq::L_UPPERARM, mq::L_FOREARM, 1.0f32),
            (mq::R_UPPERARM, mq::R_FOREARM, 1.0),
            (mq::L_THIGH, mq::L_CALF, -1.0),
            (mq::R_THIGH, mq::R_CALF, -1.0),
        ] {
            let (Some(hp), Some(hc)) = (bodies[parent], bodies[child]) else {
                continue;
            };
            let over = past_stop(
                hinge_bend(&self.posed, parent, child, sign),
                hinge_flex(child),
            );
            if over == 0.0 {
                continue;
            }
            let rp = self.posed[parent].0;
            // The hinge's axis in the world (a turn about it by +sign flexes the joint).
            let n = scale(apply_m(&rp, [1.0, 0.0, 0.0]), sign);
            let (Some(bp), Some(bc)) = (self.world.bodies.get(hp), self.world.bodies.get(hc))
            else {
                continue;
            };
            let (wp, wc) = (v3(bp.angvel()), v3(bc.angvel()));
            let (mp, mc) = (bp.mass().max(1e-3), bc.mass().max(1e-3));
            // Its spin about the hinge, toward flexing: positive.
            let c = dot(sub(wc, wp), n);
            // Turned back a share of the way each step, and no spin further past the stop: past
            // straight it is flexed back, past its most flex unflexed.
            let back = -over * RAGDOLL_HINGE_SHARE / STEP;
            let fix = if over < 0.0 {
                (back - c).max(0.0)
            } else {
                (back - c).min(0.0)
            };
            let (kc, kp) = (mp / (mp + mc), mc / (mp + mc));
            if let Some(b) = self.world.bodies.get_mut(hc) {
                b.set_angvel(vec(add(wc, scale(n, fix * kc))), true);
                // Past straight by more than the allowance: turned back onto the stop about its
                // joint (each ragdoll body's origin is its joint), so a hard fall cannot drive it
                // further than a step's motion.
                if over < 0.0 {
                    let turn = rotation(scale(n, -over));
                    let now = m3_of(*b.rotation());
                    let fixed = mul(&turn, &now);
                    b.set_rotation(quat_of(&fixed), true);
                    self.posed[child].0 = fixed;
                }
            }
            if let Some(b) = self.world.bodies.get_mut(hp) {
                b.set_angvel(vec(sub(wp, scale(n, fix * kp))), true);
            }
        }
    }

    /// The animation's facing (flat): its root's forward (the host's `facing`).
    fn anim_facing(&self) -> V3 {
        apply_m(&rotation([0.0, 0.0, self.heading]), FORWARD)
    }

    /// The host's `reach_arm`: the arm reaching as a person's does (`reach_arm_ik`).
    fn reach_arm(
        &self,
        anim: &[Xf; COUNT],
        side: u8,
        target: V3,
        w: f32,
    ) -> Option<(usize, M3, usize, M3)> {
        let u = mq::bone_of(Role::UpperArm, side)?;
        let f = mq::bone_of(Role::Forearm, side)?;
        let h = mq::bone_of(Role::Hand, side)?;
        let hand0 = anim[h].1;
        let goal = add(hand0, scale(sub(target, hand0), w.clamp(0.0, 1.0)));
        let (r1, r2) = reach_arm_ik(anim[u].1, anim[f].1, hand0, goal, self.anim_facing(), side)?;
        Some((u, r1, f, r2))
    }

    /// A brace along `dir` against an obstacle `wall` units ahead (its top at `top`), pushed at
    /// `v` m/s: its surface and which arms brace (`ragdoll_core::brace::bracing_arms`: an armed
    /// body's free arm; its weapon arm too only on a push past what one arm holds, or when the
    /// free arm cannot reach).
    fn new_brace(&mut self, dir: V3, v: f32, wall: Option<f32>, top: Option<f32>) -> Brace {
        let anim = self.anim();
        let surface = Surface::new(anim[mq::PELVIS].1, dir, wall, top);
        let mut arms = [false; 2];
        let mut reaches = [false; 2];
        for side in 0..2u8 {
            arms[side as usize] = self.rig.limbs.arm(side as usize);
            if let (Some(u), Some(f), Some(h)) = (
                mq::bone_of(Role::UpperArm, side),
                mq::bone_of(Role::Forearm, side),
                mq::bone_of(Role::Hand, side),
            ) {
                let reach = length(sub(anim[f].1, anim[u].1)) + length(sub(anim[h].1, anim[f].1));
                let target = brace_target(anim[u].1, dir, wall, top);
                reaches[side as usize] = surface.is_none()
                    || hands::reaches(anim[u].1, target, reach, self.anim_facing(), side);
                // Never behind the back, nor on an obstacle right behind the hips (the host's
                // too): the hips lean on it.
                if !hands::hand_may_brace(
                    anim[mq::PELVIS].1,
                    self.anim_facing(),
                    target,
                    surface.map(|s| s.face),
                ) {
                    arms[side as usize] = false;
                }
            }
        }
        let one_arm = self.tune.brace_holds * self.tune.balance.max(1.0) / 100.0 * hands::ONE_ARM;
        let bracing = hands::bracing_arms(arms, self.armed, v, one_arm, reaches);
        self.stats.brace_hands = bracing;
        // A hand still planted from the brace before keeps its plant (the host's too).
        let old = self.rig.brace.map_or([None; 2], |b| b.hands);
        let kept = [0u8, 1].map(|s| hands::carried_hand(old[s as usize], bracing[s as usize], s));
        for s in 0..2 {
            if kept[s] != old[s] || kept[s].is_none() {
                self.watch[s] = HandWatch::default();
                self.hand_lock[s] = None;
            }
        }
        Brace {
            dir,
            t: 0.0,
            wall,
            top,
            surface,
            hands: kept,
            facing: self.anim_facing(),
            walked: 0.0,
        }
    }

    fn brace_frame(&mut self, anim: &[Xf; COUNT], out: &mut [Option<M3>], dt: f32) {
        let Some(mut b) = self.rig.brace.take() else {
            return;
        };
        b.t += dt;
        // The planted hands let go once the body has recovered (`ragdoll_core::brace::recovered`).
        let com = self.rig.body.as_ref().map_or(0.0, |body| body.com_speed());
        let since_hit = (self.t - self.rig.hit_at) as f32;
        let turned = hands::turned(b.facing, self.anim_facing());
        // Yielded to its animation: the hands let go.
        let let_go = if self.rig.yield_at.is_some() {
            Some("its animation moved on")
        } else {
            hands::let_go(
                self.rig.balance.is_some(),
                b.t,
                com,
                since_hit,
                turned,
                b.walked,
            )
        };
        let posed = self.posed;
        let mut said = Vec::new();
        for side in 0..2u8 {
            let Some(hand) = b.hands[side as usize].as_mut() else {
                continue;
            };
            let (Some(u), Some(f), Some(h)) = (
                mq::bone_of(Role::UpperArm, side),
                mq::bone_of(Role::Forearm, side),
                mq::bone_of(Role::Hand, side),
            ) else {
                continue;
            };
            // The shown arm (the frame before): where its wrist is against the surface.
            let reach = length(sub(posed[f].1, posed[u].1)) + length(sub(posed[h].1, posed[f].1));
            // Inside the shoulder's human range (never up behind the back).
            let target = hands::held_goal(
                anim[u].1,
                brace_target(anim[u].1, b.dir, b.wall, b.top),
                self.anim_facing(),
                side,
            );
            let free = self.free_wrist[side as usize].unwrap_or(posed[h].1);
            let home = self.home(&posed, side).map_or(free, |x| x.1);
            match hand.update(
                b.surface.as_ref(),
                posed[h].1,
                home,
                free,
                posed[u].1,
                reach,
                target,
                b.t,
                dt,
                let_go,
            ) {
                Some(hands::Event::Planted(at)) => said.push(format!(
                    "{} hand plants at ({:.0}, {:.0}, {:.0})",
                    if side == 0 { "left" } else { "right" },
                    at[0],
                    at[1],
                    at[2]
                )),
                Some(hands::Event::Released(why)) => said.push(format!(
                    "{} hand lets go: {why}",
                    if side == 0 { "left" } else { "right" }
                )),
                None => {}
            }
            // The arm's motors reach for where the shown hand goes (its path, its plant: from the
            // shoulder, carried onto the animation's), so the physical arm, its pin and the shown
            // arm agree (its way back home too, so the physical arm comes home with the shown one);
            // without a path yet, toward the brace's target.
            let (target, w) = match hand.motor_goal() {
                Some(g) => (add(anim[u].1, sub(g, posed[u].1)), hand.plant_weight()),
                None => (target, hand.reach_weight(b.t)),
            };
            if let Some((ui, ur, fi, fr)) = self.reach_arm(anim, side, target, w) {
                out[ui] = Some(ur);
                out[fi] = Some(fr);
            }
        }
        for line in said {
            self.note(line);
        }
        if b.hands.iter().all(|h| h.is_none_or(|h| h.done())) {
            self.note("lets go of the wall".into());
            return;
        }
        self.rig.brace = Some(b);
    }

    /// True when the ragdoll has it.
    fn fall_frame(&mut self, out: &mut [Option<M3>], dt: f32) -> bool {
        let Some(mut f) = self.rig.fall.take() else {
            return false;
        };
        let t = self.tune;
        self.rig.balance = None;
        f.t += dt;
        // It goes over physically (the support fades over the lead, `pose_frame`); a windmill is
        // the one pose it is given.
        let across = unit(cross([0.0, 0.0, 1.0], f.dir)).unwrap_or([1.0, 0.0, 0.0]);
        for side in 0..2u8 {
            if f.windmill && self.rig.limbs.arm(side as usize) {
                if let Some(u) = mq::bone_of(Role::UpperArm, side) {
                    let phase = if side == 0 { 0.0 } else { std::f32::consts::PI };
                    let a = t.windmill_swing
                        * (std::f32::consts::TAU * t.windmill_hz * f.t + phase).sin();
                    // Inside the shoulder's human range (never up behind the back).
                    let dir = mq::bone_of(Role::Forearm, side).map_or([0.0, 0.0, -1.0], |fo| {
                        sub(self.posed[fo].1, self.posed[u].1)
                    });
                    out[u] = Some(ragdoll_core::flinch::held_turn(
                        &rotation(scale(across, a)),
                        dir,
                        self.anim_facing(),
                        side,
                    ));
                }
            }
        }
        let go = add(
            self.rig.shift,
            scale(f.dir, (f.speed * dt * UNITS_PER_METRE).min(t.fall_shift)),
        );
        if length(go) <= t.fall_shift * 2.0 {
            self.rig.shift = go;
        }
        if f.t < f.lead {
            self.rig.fall = Some(f);
            return false;
        }
        self.note(format!(
            "into its ragdoll after {:.2} s of going over{} ({:.2} m/s along ({:.2}, {:.2}))",
            f.t,
            if f.windmill { ", windmilling" } else { "" },
            f.speed,
            f.dir[0],
            f.dir[1]
        ));
        // Still going over as the knock-down is asked: a refusal eases it back up
        // (`fall_failed`).
        self.rig.fall = Some(f);
        match f.fold {
            // Over the top: launched up enough that the hips clear it (`fold_launch`).
            Some(top) => {
                let (d, s) = obstacle::fold_launch(
                    f.dir,
                    f.speed,
                    self.posed[mq::PELVIS].1[2],
                    top,
                    self.scale,
                );
                self.knock_down(d, s, f.why);
            }
            None => self.knock_down(f.dir, f.speed, f.why),
        }
        if self.in_ragdoll() {
            self.rig.fall = None;
            return true;
        }
        // Refused: posed this frame as it stands back up.
        false
    }

    /// Stage 3's entry for a knockdown: false when the layer is off (the caller knocks it down).
    fn fall(&mut self, point: V3, direction: V3, speed: f32) -> bool {
        if self.tune.hit_reactions <= 0.0 && self.tune.balance <= 0.0 {
            return false;
        }
        if self.rig.fall.is_some() {
            return true;
        }
        let dir = unit([direction[0], direction[1], 0.0]).unwrap_or([0.0, 1.0, 0.0]);
        // A waist-high obstacle close behind: it folds over it.
        let pelvis = self.anim()[mq::PELVIS].1;
        let floor_z = world::ground_point(&self.world, pelvis).map_or(0.0, |g| g[2]);
        let reach = obstacle::FOLD_REACH * self.scale * UNITS_PER_METRE;
        let fold = obstacle::scan(
            &mut Probe(&self.world),
            pelvis,
            floor_z,
            dir,
            reach,
            self.scale,
        )
        .filter(|o| o.kind == obstacle::Kind::Waist)
        .map(|o| o.top);
        self.rig.fall = Some(Fall {
            dir,
            speed,
            point,
            t: 0.0,
            lead: self.tune.fall_lead,
            windmill: false,
            why: if fold.is_some() {
                "folded over a waist-high obstacle"
            } else {
                "knockdown"
            },
            fold,
        });
        self.rig.active = true;
        self.rig.since = self.t;
        true
    }

    // --- Stage 2's frame (the host's `balance_frame`). -------------------------------------------

    fn balance_frame(&mut self, dt: f32) -> Vec<Option<M3>> {
        let mut out: Vec<Option<M3>> = vec![None; COUNT];
        let Some(mut s) = self.rig.balance.take() else {
            return out;
        };
        self.rig.shift_back = None;
        let scale_ = self.scale;
        let anim = self.anim();
        if s.outcome == Outcome::Ledge
            && s.current + 1 >= s.steps.len()
            && s.steps.last().is_some_and(|l| s.t > l.time * 0.5)
        {
            let pelvis = anim[mq::PELVIS].1;
            self.note(format!(
                "steps off a ledge: windmills, then falls ({:.2} m/s)",
                s.v
            ));
            self.rig.fall = Some(Fall {
                dir: s.dir,
                speed: s.v,
                point: pelvis,
                t: 0.0,
                lead: self.tune.ledge_lead,
                windmill: true,
                why: "stepped off a ledge",
                fold: None,
            });
            self.rig.shift = s.shift;
            return out;
        }
        if let Some((foot, at)) = s.advance(dt) {
            let reference = self.pos;
            let to = s.follow_feet();
            let to = [
                to[0],
                to[1],
                to[2].clamp(reference[2] - 60.0, reference[2] + 60.0),
            ];
            let moved = sub(to, reference);
            self.pos = to;
            self.own_moved = add(self.own_moved, moved);
            self.own_carry = add(self.own_carry, moved);
            s.reference = to;
            s.shift = sub(s.shift, moved);
            self.stats.steps += 1;
            self.note(format!(
                "{} foot lands at ({:.0}, {:.0}, {:.0}), step {} of {}; the actor follows the \
                 feet {:.0} units",
                if foot == 0 { "left" } else { "right" },
                at[0],
                at[1],
                at[2],
                s.current,
                s.steps.len(),
                length(moved)
            ));
        }
        // The animated pose after the reference moved.
        let anim = self.anim();
        let pelvis = anim[mq::PELVIS].1;
        let target = s.shift_target(pelvis, scale_);
        s.follow(target, dt);
        let shift = s.shift;
        let feet = s.feet(scale_);
        for side in 0..2u8 {
            let (Some(t), Some(c), Some(f)) = (
                mq::bone_of(Role::Thigh, side),
                mq::bone_of(Role::Calf, side),
                mq::bone_of(Role::Foot, side),
            ) else {
                continue;
            };
            let (hip0, knee0, ankle0) = (anim[t].1, anim[c].1, anim[f].1);
            let (l1, l2) = (length(sub(knee0, hip0)), length(sub(ankle0, knee0)));
            if l1 < 1.0 || l2 < 1.0 {
                continue;
            }
            let (hip, ankle) = (add(hip0, shift), add(ankle0, shift));
            let target = add(ankle, scale(sub(feet[side as usize], ankle), s.weight));
            // Never into the floor (`active::feet_on_floor`, the host's too).
            let target = active::feet_on_floor([target; 2], [ankle0; 2], [None; 2])[0];
            // The knee forward whichever way the push goes (`leg_ik`: never bent backwards).
            let Some((r1, m, level)) = leg_ik(hip0, knee0, ankle0, hip, target, self.anim_facing())
            else {
                continue;
            };
            out[t] = Some(r1);
            out[c] = Some(m);
            out[f] = Some(level);
        }
        // No scripted arms: the physical body's arms swing out of their own as it steps.
        self.rig.shift = shift;
        if s.recovering && s.outcome == Outcome::Overrun && self.rig.fall.is_none() {
            self.note(format!(
                "the steps could not catch it: falls ({:.2} m/s)",
                s.v * 0.6
            ));
            self.rig.fall = Some(Fall {
                dir: s.dir,
                speed: s.v * 0.6,
                point: pelvis,
                t: 0.0,
                lead: self.tune.overrun_lead,
                windmill: false,
                why: "more than its steps could catch",
                fold: None,
            });
            return out;
        }
        if s.done(self.t) {
            self.note(format!(
                "steps done ({:?}): back to its animation (the pelvis's {:.0} units eased out)",
                s.outcome,
                length(self.rig.shift)
            ));
            self.rig.shift_back = Some((self.rig.shift, self.t, SHIFT_BACK));
        } else {
            self.rig.balance = Some(s);
        }
        out
    }

    // --- The frame (the host's `active_ragdoll::apply`). -----------------------------------------

    /// One step of the simulation.
    pub fn step(&mut self) {
        tuning::set(self.tune);
        // The standing body's frame time: the game's varies (its frame rate); the ragdoll's
        // world keeps [`STEP`].
        let dt = if self.state_is_standing() {
            self.dt
        } else {
            STEP
        };
        self.t += dt as f64;
        self.condition.decay(dt);
        if self.state_is_standing() {
            let moving = self.moving();
            let mut turn = if moving { self.turn * dt } else { 0.0 };
            if let Some(by) = self.turn_by {
                turn = turn.clamp(-(by - self.turned).max(0.0), (by - self.turned).max(0.0));
            }
            self.turned += turn.abs();
            self.heading += turn;
            let walk = if moving { self.walk * dt } else { 0.0 };
            self.pos[0] += walk;
            self.carry([walk, 0.0, 0.0]);
            self.rig.moved_since_hit += walk.abs();
            self.rig.turned_since_hit += turn.abs();
            if active::yields(self.rig.moved_since_hit, self.rig.turned_since_hit) {
                self.yield_now("its AI moves it");
            }
            // The gait: in while it moves (walking or turning), out after.
            let going = moving && (self.walk != 0.0 || turn != 0.0);
            let to = if going { 1.0 } else { 0.0 };
            self.gait_w += (to - self.gait_w).clamp(-dt / GAIT_IN, dt / GAIT_IN);
            self.gait_phase += dt * std::f32::consts::TAU * 1.1;
            let turning = if turn != 0.0 { turn.signum() } else { 0.0 };
            self.turning += (turning - self.turning).clamp(-dt / GAIT_IN, dt / GAIT_IN);
            if self.read_back {
                self.read_bases(dt);
            }
        }
        self.posed_before = self.posed;
        match self.state {
            State::Standing => self.pose_frame(dt),
            State::Ragdoll { .. } => {
                self.ragdoll_frame();
                // And after its step: what the step drove past a stop is held there before it is
                // shown.
                if let State::Ragdoll { bodies, .. } = &self.state {
                    let handles = bodies.clone();
                    self.stop_ragdoll_hinges(&handles);
                }
            }
            State::GettingUp { .. } => self.get_up_frame(dt),
        }
        let bent = back_bend(&self.posed);
        if bent > self.stats.back_bend {
            self.stats.back_bend = bent;
        }
        if self.stats.recovered_at.is_none() && self.stats.last_hit.is_some() && self.recovered() {
            self.stats.recovered_at = Some(self.t);
            let since = self.t - self.stats.first_hit.unwrap_or(self.t);
            self.note(format!("recovered: {since:.2} s after the first hit"));
        }
    }

    /// The game's own move of the actor (its AI walking it: not the steps') carried by the
    /// physical body (the host's `carry`): its origin goes with the actor, so the body never trails
    /// the actor on its pelvis spring and snaps back to it as it is handed back. Not a body going
    /// over (that is physical).
    fn carry(&mut self, moved: V3) {
        if self.rig.body.is_none() || self.rig.fall.is_some() || length(moved) <= 0.0 {
            return;
        }
        if let Some(b) = self.rig.brace.as_mut() {
            b.walked += length(moved);
        }
        if let Some(s) = self.rig.balance.as_mut() {
            s.carry(moved);
        }
        if let Some((feet, _)) = self.rig.planted.as_mut() {
            for f in feet.iter_mut() {
                *f = add(*f, moved);
            }
        }
    }

    /// The body dropped as not believable on the next frame (as `Body::check` or the host's
    /// `pose::sane` do): for the tests of the way back.
    pub fn drop_body(&mut self, why: &str) {
        if self.rig.body.is_some() {
            self.rig.failed = Some(why.to_string());
        }
    }

    /// Back on its feet and at rest.
    pub fn recovered(&self) -> bool {
        self.standing() && !self.rig.active
    }

    fn pose_frame(&mut self, dt: f32) {
        if let Some(fade) = self.rig.fade.as_mut() {
            if !fade.advance(dt) {
                self.rig.fade = None;
            }
        }
        let mut overrides: Vec<Option<M3>> = vec![None; COUNT];
        if self.rig.active {
            overrides = self.balance_frame(dt);
            // The legs keep the steps' last targets while the body goes over (no jump in them).
            if self.rig.balance.is_some() {
                self.rig.held = overrides.clone();
            } else if self.rig.fall.is_some() {
                for (b, o) in overrides.iter_mut().enumerate() {
                    if o.is_none() && !mq::role_side(b).0.is_arm() {
                        *o = self.rig.held.get(b).copied().flatten();
                    }
                }
            }
            let anim = self.anim();
            self.brace_frame(&anim, &mut overrides, dt);
            // The grip hold: the support arm's motors on the grip too (the physical arm stays on
            // the weapon; once the brace hand is handed back or the brace is over).
            let bracing = self
                .rig
                .brace
                .and_then(|b| b.hands[0])
                .is_some_and(|h| !h.released());
            if self.grip_hold && !bracing {
                if let (Some(grip), Some(u)) =
                    (self.grip(&self.posed), mq::bone_of(Role::UpperArm, 0))
                {
                    let goal = add(anim[u].1, sub(grip.1, self.posed[u].1));
                    if let Some((ui, ur, fi, fr)) = self.reach_arm(&anim, 0, goal, 1.0) {
                        overrides[ui] = Some(ur);
                        overrides[fi] = Some(fr);
                    }
                }
            }
            if self.fall_frame(&mut overrides, dt) {
                return;
            }
        }
        let anim = self.anim();
        // The targets: the animation, with the steps' legs, the brace's arms, the windmill (each
        // override a world turn carried down its chain).
        let mut chain = [IDENTITY; COUNT];
        let mut target = [IDENTITY; COUNT];
        for b in 0..COUNT {
            let up = mq::PARENT[b].map_or(IDENTITY, |p| chain[p]);
            chain[b] = match overrides[b] {
                Some(o) => mul(&up, &o),
                None => up,
            };
            target[b] = mul(&chain[b], &anim[b].0);
        }
        // The steps' pelvis offset, eased out once they are done (the host's `SHIFT_BACK`).
        if self.rig.balance.is_none() && self.rig.fall.is_none() {
            if self.rig.shift_back.is_none() && length(self.rig.shift) > 0.0 {
                self.rig.shift_back = Some((self.rig.shift, self.t, SHIFT_BACK));
            }
            if let Some((from, since, over)) = self.rig.shift_back {
                let k = active::ease((self.t - since) as f32 / over);
                if k >= 1.0 {
                    self.rig.shift_back = None;
                }
                self.rig.shift = scale(from, 1.0 - k);
            }
        }
        let pelvis_target = add(anim[mq::PELVIS].1, self.rig.shift);
        let before = self.posed;
        let busy =
            self.rig.balance.is_some() || self.rig.brace.is_some() || self.rig.fall.is_some();
        if busy {
            self.rig.since = self.t;
        }
        let support_now = self.support();
        let facing_now = self.anim_facing();
        let shown_w = match self.rig.body.as_mut() {
            Some(body) => {
                let m = 1.0 / UNITS_PER_METRE;
                // The feet: where the steps put them (swinging, then planted); after the steps,
                // eased back to the animation's.
                let animated = [anim[mq::L_FOOT].1, anim[mq::R_FOOT].1];
                let planted = match &self.rig.balance {
                    Some(s) => {
                        let f = s.feet(self.scale);
                        self.rig.planted = Some((f, self.t));
                        f
                    }
                    None => match self.rig.planted {
                        // After the steps, eased back to the animation's (the host's `FEET_BACK`).
                        Some((p, since)) => {
                            let k = active::ease((self.t - since) as f32 / FEET_BACK);
                            [0, 1].map(|i| add(p[i], scale(sub(animated[i], p[i]), k)))
                        }
                        None => animated,
                    },
                };
                // Never into the floor (the host's too): at least the animation's own foot.
                let planted = active::feet_on_floor(planted, animated, [None; 2]);
                let origin = self.rig.origin;
                let to_body = |p: V3| scale(sub(p, origin), m);
                let feet: Vec<(usize, V3)> = vec![
                    (mq::L_FOOT, to_body(planted[0])),
                    (mq::R_FOOT, to_body(planted[1])),
                ];
                let support = support_now;
                let pins: Vec<(usize, V3, f32, V3)> = self
                    .rig
                    .brace
                    .iter()
                    .flat_map(|br| br.hands.iter().flatten())
                    .filter_map(|h| {
                        let (at, share, normal) = h.pin()?;
                        Some((mq::bone_of(Role::Hand, h.side)?, to_body(at), share, normal))
                    })
                    .collect();
                let targets = active::Targets {
                    rot: target.iter().map(active::qfrom_m).collect(),
                    pelvis: to_body(pelvis_target),
                    feet,
                    // A brace hand's wrist pinned along its path and where it touched
                    // (`ragdoll_core::brace::Hand::pin`).
                    hands: pins.iter().map(|p| (p.0, p.1)).collect(),
                    hand_share: pins.iter().map(|p| p.2).collect(),
                    hand_normal: pins.iter().map(|p| p.3).collect(),
                    support,
                    // An armed body keeps its aim: its arms track their world rotations (the
                    // torso flinches under them).
                    world: (0..COUNT)
                        .map(|b| {
                            let (role, side) = mq::role_side(b);
                            if role.is_arm() && side < 2 {
                                // The aim kept, or the brace's reach to the wall point (not
                                // riding up with a torso pitched toward the wall); an armed
                                // body's weapon arm on its aim while its free arm braces.
                                let side = side as usize;
                                let (brace, aims) = self.rig.brace.map_or((0.0, false), |br| {
                                    (br.weight(side), br.aims(side, self.armed))
                                });
                                if aims {
                                    1.0
                                } else {
                                    (self.rig.aim_hold * 1.25)
                                        .max(brace * ragdoll_core::tuning::get().brace_world)
                                        .min(1.0)
                                }
                            } else {
                                0.0
                            }
                        })
                        .collect(),
                    anim: anim.iter().map(|a| active::qfrom_m(&a.0)).collect(),
                    forward: facing_now,
                };
                // A hurt leg sags: its knee bent more, the pelvis lower over it.
                let mut targets = targets;
                for side in 0..2usize {
                    let (knee, drop) = self.condition.sag(side);
                    if knee > 0.0 {
                        if let Some(c) = mq::bone_of(Role::Calf, side as u8) {
                            body.flex(&mut targets.rot, c, knee);
                        }
                        targets.pelvis[2] -= drop * 0.5;
                    }
                }
                let since_hit = (self.t - self.rig.hit_at) as f32;
                // A deliberate brace: the bracing arms' motors firmer while they reach and hold;
                // an armed body's weapon arm held firm on its aim.
                if let Some(br) = self.rig.brace {
                    for (b, d) in body.dip.iter_mut().enumerate() {
                        let (role, side) = mq::role_side(b);
                        if role.is_arm() && side < 2 {
                            let side = side as usize;
                            if br.aims(side, self.armed) {
                                *d = d.min(-AIM_FIRM);
                            } else {
                                let w = br.weight(side);
                                *d = d.min(-ragdoll_core::tuning::get().brace_firm * w);
                            }
                        }
                    }
                }
                // A hurt arm's motors weak (it dangles); fatigue weakens them all.
                for b in 0..COUNT {
                    let (role, side) = mq::role_side(b);
                    if role.is_arm() && side < 2 && self.rig.brace.is_none() {
                        let d = self.condition.dangle(side as usize);
                        if d > 0.0 {
                            body.dip[b] = body.dip[b].max(d);
                        }
                    }
                }
                let strength = active::strength(since_hit) * self.condition.motor_scale();
                // Carried along with the animation's own motion (its AI's walk and turn, its gait),
                // the steps' own moves of the actor left out (`Body::carry`).
                let frames: Vec<(active::Q, V3)> =
                    anim.iter().map(|a| (active::qfrom_m(&a.0), a.1)).collect();
                let own = std::mem::take(&mut self.own_carry);
                if let (Some(from), false) = (self.rig.carry_from.as_ref(), self.rig.fall.is_some())
                {
                    let to_body = |p: V3| scale(sub(p, origin), m);
                    let mut from: Vec<_> = from.iter().map(|&(q, p)| (q, to_body(p))).collect();
                    let to: Vec<_> = frames
                        .iter()
                        .map(|&(q, p)| (q, to_body(sub(p, own))))
                        .collect();
                    // The animation jumped: the jump is not carried into the body, and the bounds'
                    // kept offsets move onto the new animation (the host's too).
                    if std::mem::take(&mut self.rebase_legs) {
                        for b in 0..COUNT {
                            self.rig.limit.rebase(b, from[b].0, to[b].0);
                            from[b] = to[b];
                        }
                    }
                    body.carry(&from, &to);
                }
                self.rig.carry_from = Some(frames);
                body.free_arms(self.rig.brace.is_none() && self.rig.fall.is_none());
                body.step(&targets, dt, strength);
                if let Err(why) = body.check() {
                    self.rig.failed = Some(why);
                }
                // Handed back by its time whatever still holds it, unless it goes over
                // (`active::reaction_weight`).
                active::reaction_weight(
                    (self.t - self.rig.since) as f32,
                    since_hit,
                    self.rig.fall.is_some(),
                    self.rig.yield_at.map(|y| (self.t - y) as f32),
                )
            }
            None => 0.0,
        };
        if self.rig.body.is_some() && shown_w <= 0.0 && self.stats.handed_back_at.is_none() {
            self.stats.handed_back_at = Some(self.t);
        }
        // A body gone wrong is dropped back to the animation, said once (the game's too).
        // Never in one frame: the pose last shown eases out over the animation (the host's too),
        // the animation's, not this frame's targets (the steps' legs went with the body).
        let dropped = self.rig.failed.is_some();
        if let Some(why) = self.rig.failed.take() {
            self.note(format!(
                "the active ragdoll is dropped back to the animation: {why}; eased back over {} s",
                active::FADE_BACK
            ));
            self.stats.dropped += 1;
            self.rig.body = None;
            self.rig.balance = None;
            self.rig.brace = None;
            self.rig.fall = None;
            self.rig.shift = [0.0; 3];
            self.rig.shift_back = None;
            self.rig.fade = self
                .rig
                .last
                .take()
                .map(|(off, p)| active::Fade::new(off, p));
        }
        // The shown pose: the targets and the body's, blended by the body's weight (eased).
        let mut rots = target;
        let mut pelvis = pelvis_target;
        if dropped {
            rots = std::array::from_fn(|b| anim[b].0);
            pelvis = anim[mq::PELVIS].1;
        }
        if let Some(body) = &self.rig.body {
            let pose = body.pose();
            // The targets as the body tracks them (no elbow or knee past its human range) and the
            // body, blended (`Body::shown`).
            let wanted: Vec<active::Q> = target.iter().map(active::qfrom_m).collect();
            let mut shown = body.shown(&wanted, shown_w);
            // The drawn flinch on the torso (`active::draw_flinch`, the host's too).
            self.rig.flinches.retain_mut(|f| f.advance(dt));
            let roles: Vec<Role> = (0..COUNT).map(|b| mq::role_side(b).0).collect();
            self.rig.torso_keep =
                active::torso_keep(self.rig.torso_keep, self.rig.brace.is_some(), dt);
            active::draw_flinch(
                &mut shown,
                &wanted,
                &active::flinch_places(&roles),
                &self.rig.flinches,
                shown_w,
                self.rig.torso_keep,
            );
            let sim = add(scale(pose[mq::PELVIS].1, UNITS_PER_METRE), self.rig.origin);
            let mut off = scale(sub(sim, pelvis_target), shown_w);
            // Handed back: it only settles (`active::Settle`), its ceilings coming down with
            // the weight; a brace's arms go home their own way.
            let bracing = self.rig.brace.is_some();
            let free = |b: usize| bracing && mq::role_side(b).0.is_arm();
            if shown_w < 1.0 && self.rig.fall.is_none() {
                let keep = if self.rig.shown_w <= 0.0 {
                    1.0
                } else {
                    shown_w / self.rig.shown_w
                };
                self.rig
                    .settle
                    .hold_down(&mut shown, &wanted, &mut off, keep, free);
            } else {
                self.rig.settle.open();
            }
            // Never more than the push justifies, nor turning faster than a person
            // (`active::Limit`); a fall is the ragdoll's.
            if self.rig.fall.is_none() && !self.limit_off {
                self.stats.limited +=
                    self.rig
                        .limit
                        .hold(&mut shown, &wanted, &mut off, self.rig.react_v, dt, free);
            } else {
                self.rig.limit.follow(&shown, &wanted, off);
            }
            self.rig.shown_w = shown_w;
            for (b, q) in shown.into_iter().enumerate() {
                rots[b] = active::m_from_q(q);
            }
            pelvis = add(pelvis_target, off);
        }
        // What is left of a dropped body's last pose, on top.
        if let Some(fade) = &self.rig.fade {
            let mut off = sub(pelvis, anim[mq::PELVIS].1);
            fade.on(&mut rots, &mut off);
            pelvis = add(anim[mq::PELVIS].1, off);
        }
        // The pelvis never more than a few units under its animation (the host's too).
        if self.rig.fall.is_none() && !self.limit_off {
            let off = sub(pelvis, anim[mq::PELVIS].1);
            pelvis = add(
                anim[mq::PELVIS].1,
                active::hold_pelvis_up(off, self.rig.balance.is_some()),
            );
            // A shown foot never under its animated one (`active::lift_feet`, the host's too).
            let at: Vec<V3> = self.place(&rots, pelvis).iter().map(|x| x.1).collect();
            let legs = [
                (mq::L_THIGH, mq::L_CALF, mq::L_FOOT),
                (mq::R_THIGH, mq::R_CALF, mq::R_FOOT),
            ];
            // Eased in and out (`active::lift_amount`); none wanted while the steps place the
            // legs (their targets are held up already).
            let feet = [mq::L_FOOT, mq::R_FOOT];
            let stepping = self.rig.balance.is_some();
            let lift = [0, 1].map(|i| {
                let f = feet[i];
                let excess = if stepping {
                    0.0
                } else {
                    anim[f].1[2] - active::LIFT_SLACK - at[f][2]
                };
                active::lift_amount(self.lift_low[i], excess, dt)
            });
            self.lift_low = lift;
            let low = [0, 1].map(|i| at[feet[i]][2] + lift[i]);
            active::lift_feet(&mut rots, &at, &legs, &low, self.anim_facing());
        }
        let mut out = self.place(&rots, pelvis);
        // The free arms' wrists (before the brace's IK: where a released hand goes back to).
        for side in 0..2u8 {
            self.free_wrist[side as usize] = mq::bone_of(Role::Hand, side).map(|h| out[h].1);
        }
        // A planted hand stays where it touched: the shown arm put on it (`brace::plant_arm`).
        if let Some(br) = self.rig.brace {
            let mut moved = false;
            for hand in br.hands.iter().flatten() {
                let (Some(goal), w) = (hand.goal(), hand.plant_weight()) else {
                    continue;
                };
                // Let go: on its way back home, the home as this frame has it (no lag behind the
                // weapon hand that carries a grip).
                let goal = match (hand.home_offset(), self.home(&out, hand.side)) {
                    (Some(off), Some(home)) => add(home.1, off),
                    _ => goal,
                };
                let (Some(u), Some(f), Some(h)) = (
                    mq::bone_of(Role::UpperArm, hand.side),
                    mq::bone_of(Role::Forearm, hand.side),
                    mq::bone_of(Role::Hand, hand.side),
                ) else {
                    continue;
                };
                // Solved from the arm as it was shown a frame before (on the shoulder where it is
                // now), so the shown arm moves on from itself, whatever the physical arm under it
                // does; blended with the free arm by the plant's weight.
                let (pu, pf, ph) = (self.posed[u].0, self.posed[f].0, self.posed[h].0);
                let shoulder = out[u].1;
                let elbow = add(shoulder, sub(self.posed[f].1, self.posed[u].1));
                let wrist = add(elbow, sub(self.posed[h].1, self.posed[f].1));
                if let Some((r1, r2)) = hands::plant_arm(
                    shoulder,
                    elbow,
                    wrist,
                    goal,
                    1.0,
                    self.anim_facing(),
                    hands::MOST_TWIST * self.dt,
                ) {
                    let (iu, ifo) = (mul(&r1, &pu), mul(&mul(&r1, &r2), &pf));
                    // The hand with the arm: carried by its forearm while it reaches, held as it
                    // touched once planted (the palm stays on the surface).
                    let carried = mul(&ifo, &mul(&transpose(&pf), &ph));
                    let ih = if hand.planted() {
                        *self.hand_lock[hand.side as usize].get_or_insert(ph)
                    } else if let (true, Some(home)) = (hand.released(), self.home(&out, hand.side))
                    {
                        // Turned onto its grip as it comes home (from as it let go), no faster
                        // than a hand turns from as it was drawn (the host's too).
                        let from = self.hand_lock[hand.side as usize].unwrap_or(carried);
                        let to = active::m_from_q(active::qslerp(
                            active::qfrom_m(&from),
                            active::qfrom_m(&home.0),
                            active::ease(hand.home_share()),
                        ));
                        hands::turn_toward(&ph, &to, hands::HAND_HOME_TURN * self.dt)
                    } else {
                        carried
                    };
                    let blend = |free: M3, ik: M3| {
                        active::m_from_q(active::qslerp(
                            active::qfrom_m(&free),
                            active::qfrom_m(&ik),
                            w.clamp(0.0, 1.0),
                        ))
                    };
                    rots[u] = blend(rots[u], iu);
                    rots[f] = blend(rots[f], ifo);
                    rots[h] = blend(rots[h], ih);
                    moved = true;
                }
            }
            if moved {
                out = self.place(&rots, pelvis);
            }
        }
        // The grip hold: a two-handed weapon's support hand, back on its grip after a brace, kept
        // there while the body is physical (its weight: the body's in the shown pose), so it never
        // swings off the weapon as the arm is handed back.
        if let Some(on) = self.grip_hold_weight(shown_w) {
            if let Some(grip) = self.grip(&out) {
                let (u, f, h) = (mq::L_UPPERARM, mq::L_FOREARM, mq::L_HAND);
                let (pu, pf) = (self.posed[u].0, self.posed[f].0);
                let shoulder = out[u].1;
                let elbow = add(shoulder, sub(self.posed[f].1, self.posed[u].1));
                let wrist = add(elbow, sub(self.posed[h].1, self.posed[f].1));
                if let Some((r1, r2)) = hands::plant_arm(
                    shoulder,
                    elbow,
                    wrist,
                    grip.1,
                    1.0,
                    self.anim_facing(),
                    hands::MOST_TWIST * self.dt,
                ) {
                    let blend = |a: M3, b: M3| {
                        active::m_from_q(active::qslerp(
                            active::qfrom_m(&a),
                            active::qfrom_m(&b),
                            on,
                        ))
                    };
                    rots[u] = blend(rots[u], mul(&r1, &pu));
                    rots[f] = blend(rots[f], mul(&mul(&r1, &r2), &pf));
                    rots[h] = blend(rots[h], grip.0);
                    out = self.place(&rots, pelvis);
                }
            }
        }
        // Last, the arms turned out of the torso where they went deeper than the animation has
        // them (the host's too: `Body::clear_arms`).
        // Not an arm a brace or the grip puts on its goal (that goal is held already).
        let mut skip: Vec<usize> = Vec::new();
        for side in 0..2u8 {
            let gripping = side == 0 && self.grip_hold;
            // A planted brace hand stays where it touched.
            let planted = self
                .rig
                .brace
                .and_then(|b| b.hands[side as usize])
                .is_some_and(|h| h.planted());
            // An armed body's arms are the weapon pose's and its aim's.
            if gripping || planted || self.armed {
                skip.extend(mq::bone_of(Role::Hand, side));
            }
        }
        if let Some(body) = &self.rig.body {
            let mut rots: Vec<M3> = out.iter().map(|x| x.0).collect();
            let mut joints: Vec<V3> = out.iter().map(|x| x.1).collect();
            let size = self.scale;
            body.clear_arms(
                &mut rots,
                &mut joints,
                &anim,
                |r, j, i| mq::capsule_at(i, r[i], j[i], size),
                ARM_SLACK,
                &skip,
            );
            // And inside the shoulder's human range, against the chest as it is turned.
            let chest = mul(&rots[mq::SPINE2], &transpose(&anim[mq::SPINE2].0));
            body.hold_shoulders(
                &mut rots,
                &mut joints,
                apply_m(&chest, self.anim_facing()),
                apply_m(&chest, [0.0, 0.0, 1.0]),
                &anim,
                self.anim_facing(),
                &skip,
            );
            for b in 0..COUNT {
                out[b] = (rots[b], joints[b]);
            }
        }
        self.posed = out;
        if self.read_back {
            self.write_back();
        }
        // The pose shown, over the animation: where a fade starts if the body is dropped.
        let anim_r: Vec<M3> = anim.iter().map(|a| a.0).collect();
        let out_r: Vec<M3> = out.iter().map(|a| a.0).collect();
        self.rig.last = Some((
            active::Fade::offsets(&out_r, &anim_r),
            sub(out[mq::PELVIS].1, anim[mq::PELVIS].1),
        ));
        self.watch_hands();
        self.watch_regrip();
        if self.stats.last_hit.is_some() {
            if let Some(off) = self.support_off_grip() {
                self.stats.off_grip.push(off);
            }
        }
        self.measure(&before, dt);
        if self.rig.body.is_some() && shown_w <= 0.0 && !busy {
            self.rig.body = None;
            self.rig.flinches.clear();
            self.rig.active = false;
            self.rig.shift = [0.0; 3];
            self.note("back in its animation".into());
        }
    }

    /// The shown pose's bones placed from their world rotations down from the pelvis.
    fn place(&self, rots: &[M3; COUNT], pelvis: V3) -> [Xf; COUNT] {
        let mut out = [IDENT_XF; COUNT];
        for b in 0..COUNT {
            let pos = match mq::PARENT[b] {
                Some(p) => add(out[p].1, apply_m(&out[p].0, self.rest_local(b))),
                None => pelvis,
            };
            out[b] = (rots[b], pos);
        }
        out
    }

    /// The brace's hands watched for bounce in the shown pose ([`Stats::hand_losses`],
    /// `hand_jitter`, `hand_slip`, `elbow_reversals`): a hand on the surface (its wrist within
    /// [`TOUCH`] of its rest) that comes off it while its hand still holds is a loss; once settled,
    /// its speed and slide; a planted arm's elbow turning back on itself, an oscillation.
    fn watch_hands(&mut self) {
        let Some(br) = self.rig.brace else {
            return;
        };
        let Some(surface) = br.surface else {
            return;
        };
        let dt = self.dt.max(1e-4);
        for side in 0..2usize {
            let Some(hand) = br.hands[side] else {
                continue;
            };
            let (Some(u), Some(f), Some(h)) = (
                mq::bone_of(Role::UpperArm, side as u8),
                mq::bone_of(Role::Forearm, side as u8),
                mq::bone_of(Role::Hand, side as u8),
            ) else {
                continue;
            };
            let wrist = self.posed[h].1;
            let holding = !matches!(hand.phase, hands::Phase::Released { .. });
            // The drawn wrist on its plant once it has come to rest there (world-pinned), and how
            // long a released hand takes home (its arm handed back).
            match hand.phase {
                hands::Phase::Planted {
                    at, t, approach, ..
                } if t >= approach => {
                    self.stats.plant_drift = self.stats.plant_drift.max(length(sub(wrist, at)));
                }
                // Home: locked on it (or handed back to the free arm already).
                hands::Phase::Released { t, locked, .. } => {
                    if !locked && hand.plant_weight() > 1e-3 {
                        self.stats.return_time = self.stats.return_time.max(t);
                    }
                }
                _ => {}
            }
            let w = &mut self.watch[side];
            let (gap, _) = surface.gap(wrist);
            let touching = gap <= hands::HAND + TOUCH;
            if w.touching && !touching && holding {
                self.stats.hand_losses += 1;
            }
            if touching && holding {
                if !w.touching {
                    w.since = 0.0;
                } else {
                    w.since += dt;
                }
                if w.since >= SETTLE_TOUCH {
                    let settled = *w.settled.get_or_insert(wrist);
                    let slip = length(sub(wrist, settled)) / UNITS_PER_METRE * 100.0;
                    self.stats.hand_slip = self.stats.hand_slip.max(slip);
                    if let Some(l) = w.last {
                        let v = length(sub(wrist, l)) / dt / UNITS_PER_METRE;
                        self.stats.hand_jitter = self.stats.hand_jitter.max(v);
                    }
                }
            }
            // The elbow watched once the wrist is at rest on its plant (its own way onto it is
            // the touch, not a swing).
            let at_rest = matches!(
                hand.phase,
                hands::Phase::Planted { t, approach, .. } if t >= approach
            );
            if at_rest {
                let a = sub(self.posed[f].1, self.posed[u].1);
                let c = sub(wrist, self.posed[f].1);
                let bend = (dot(a, c) / (length(a) * length(c)).max(1e-6))
                    .clamp(-1.0, 1.0)
                    .acos();
                // A turn back counts once the elbow has come back more than [`ELBOW_SWING`] from
                // the furthest it went the other way (a visible swing, not the torso's sway).
                match w.elbow {
                    None => w.elbow = Some(bend),
                    Some(extreme) => {
                        if w.elbow_dir == 0.0 {
                            if (bend - extreme).abs() > ELBOW_SWING {
                                w.elbow_dir = (bend - extreme).signum();
                                w.elbow = Some(bend);
                            }
                        } else if (bend - extreme) * w.elbow_dir > 0.0 {
                            w.elbow = Some(bend);
                        } else if (extreme - bend) * w.elbow_dir > ELBOW_SWING {
                            w.reversals += 1;
                            self.stats.elbow_reversals =
                                self.stats.elbow_reversals.max(w.reversals);
                            w.elbow_dir = -w.elbow_dir;
                            w.elbow = Some(bend);
                        }
                    }
                }
            }
            w.touching = touching && holding;
            w.last = Some(wrist);
        }
    }

    /// How far off its weapon's grip the support hand is in the shown pose (0 on it, 1 fully off:
    /// `ragdoll_core::one_hand::off_grip`); none when the body holds no weapon two-handed.
    pub fn support_off_grip(&self) -> Option<f32> {
        let grip = self.grip(&self.posed)?;
        let d = length(sub(self.posed[mq::L_HAND].1, grip.1));
        Some(ragdoll_core::one_hand::off_grip(d))
    }

    /// The grip hold's weight now (none: no hold): once a two-handed weapon's support hand is back
    /// on its grip after a brace, while the body is physical, the body's weight `shown_w` in the
    /// shown pose (the hand stays on the grip as the body blends back to the animation).
    fn grip_hold_weight(&mut self, shown_w: f32) -> Option<f32> {
        if self.grip(&self.posed).is_none() || self.rig.body.is_none() {
            self.grip_hold = false;
            return None;
        }
        if let Some(h) = self.rig.brace.and_then(|b| b.hands[0]) {
            if h.on_home() {
                self.grip_hold = true;
            }
        }
        self.grip_hold.then_some(hands::grip_weight(shown_w))
    }

    /// Where a released brace hand goes back to in the pose `pose` (rotation and wrist): a
    /// two-handed weapon's support hand to its grip ([`Sim::grip`]); else the animation's hand as
    /// the bone its arm hangs from carries it (`ragdoll_core::brace::carried`).
    pub fn home(&self, pose: &[Xf; COUNT], side: u8) -> Option<Xf> {
        if side == 0 {
            if let Some(g) = self.grip(pose) {
                return Some(g);
            }
        }
        let h = mq::bone_of(Role::Hand, side)?;
        // Carried by the chest, never by the clavicle the arm moves (the host's too).
        let mut c = mq::PARENT[mq::bone_of(Role::UpperArm, side)?]?;
        while mq::role_side(c).0.is_arm() {
            c = mq::PARENT[c]?;
        }
        let anim = self.anim();
        Some(hands::carried(anim[c], anim[h], pose[c]))
    }

    /// Where the support (left) hand grips the weapon in the pose `pose`, when it does
    /// (`ragdoll_core::brace::grips`: armed, the animation's hands close): the animation's grip
    /// as the weapon (right) hand carries it.
    pub fn grip(&self, pose: &[Xf; COUNT]) -> Option<Xf> {
        let anim = self.anim();
        let (r, l) = (anim[mq::R_HAND], anim[mq::L_HAND]);
        hands::grips(self.armed, l.1, r.1).then(|| hands::carried(r, l, pose[mq::R_HAND]))
    }

    /// The support hand going back to its grip after a brace ([`Stats::regrip_overshoot`],
    /// `regrip_reversals`, `regrip_drift`): from when its brace hand lets go until
    /// [`REGRIP_WATCH`] seconds on, its wrist against the grip as the weapon hand carries it.
    fn watch_regrip(&mut self) {
        let Some(grip) = self.grip(&self.posed) else {
            return;
        };
        let wrist = self.posed[mq::L_HAND].1;
        let rel = sub(wrist, grip.1);
        let d = length(rel);
        let w = &mut self.regrip;
        if w.done {
            return;
        }
        let released = self
            .rig
            .brace
            .and_then(|b| b.hands[0])
            .is_some_and(|h| h.released());
        match w.since {
            None => {
                if released {
                    w.since = Some(0.0);
                    w.toward = unit(scale(rel, -1.0)).unwrap_or([0.0; 3]);
                    w.least = d;
                    self.stats.regrip_watched = true;
                }
            }
            Some(t) => {
                let t = t + self.dt;
                w.since = Some(t);
                // Past the grip along its way back.
                let past = dot(rel, w.toward).max(0.0) / UNITS_PER_METRE * 100.0;
                self.stats.regrip_overshoot = self.stats.regrip_overshoot.max(past);
                if d < w.least {
                    w.least = d;
                } else if d > w.least + REGRIP_SWING {
                    self.stats.regrip_reversals += 1;
                    w.least = d;
                }
                if d <= REGRIP_LOCK {
                    w.locked = true;
                }
                if w.locked {
                    let off = (d / UNITS_PER_METRE * 100.0) - REGRIP_LOCK / UNITS_PER_METRE * 100.0;
                    self.stats.regrip_drift = self.stats.regrip_drift.max(off.max(0.0));
                }
                if t >= REGRIP_WATCH {
                    w.done = true;
                }
            }
        }
    }

    /// The physical body made from the pose shown now, moving as it moved (nothing jumps).
    fn ensure_body(&mut self) {
        if self.rig.body.is_some() {
            return;
        }
        // Its bounds from what was drawn the frame before (the host's too).
        let drawn = self.offsets_before.map_or_else(Vec::new, |o| o.to_vec());
        self.rig.limit.seed(&drawn, COUNT);
        self.rig.settle.open();
        self.rig.carry_from = None;
        self.rig.torso_keep = if self.rig.brace.is_some() {
            1.0
        } else {
            active::BODY_TORSO_SHARE
        };
        self.own_carry = [0.0; 3];
        let m = 1.0 / UNITS_PER_METRE;
        let mass = self.mass();
        let origin = self.posed[mq::PELVIS].1;
        self.rig.origin = origin;
        let bones: Vec<active::Bone> = (0..COUNT)
            .map(|b| {
                let (rot, pos) = self.posed[b];
                let end = add(
                    pos,
                    apply_m(&rot, scale(sub(mq::end(b), mq::JOINT[b]), self.scale)),
                );
                let (r0, p0) = self.posed_before[b];
                active::Bone {
                    parent: self.body_parents[b],
                    role: mq::role_side(b).0,
                    q: active::qfrom_m(&rot),
                    joint: scale(sub(pos, origin), m),
                    end: scale(sub(end, origin), m),
                    radius: mq::CAPSULE[b].2 * self.scale * m,
                    mass: mass * mq::MASS_SHARE[b],
                    before: Some((active::qfrom_m(&r0), scale(sub(p0, origin), m))),
                }
            })
            .collect();
        let mut body = active::Body::from_pose(&bones, STEP);
        body.unarmed(!self.armed);
        self.rig.body = Some(body);
    }

    /// The run's measures for a standing frame: the arms' turning speed, the weapon hand's drift
    /// off its aim, the flinch's largest turn.
    fn measure(&mut self, before: &[Xf; COUNT], dt: f32) {
        if self.stats.last_hit.is_none() || dt <= 0.0 {
            return;
        }
        // Continuity: each drawn bone's move and turn in this frame beyond its clean animation's
        // own (`clean_anim`: a forced jump is not the animation's motion), standing.
        let clean = self.clean_anim();
        // How far any drawn bone is from its animated place (a fall's and a bracing arm's own
        // aside), and every drawn bone's length against the animation's (Patrick's 21-16-09 clip).
        if self.rig.fall.is_none() {
            let anim_now = self.anim();
            for b in 0..COUNT {
                let (role, side) = mq::role_side(b);
                let bracing = role.is_arm()
                    && side < 2
                    && self
                        .rig
                        .brace
                        .is_some_and(|br| br.hands[side as usize].is_some());
                if !bracing {
                    let d = length(sub(self.posed[b].1, anim_now[b].1));
                    self.stats.drawn_most_off = self.stats.drawn_most_off.max(d);
                }
                if let Some(p) = mq::PARENT[b] {
                    // Against the bind pose (the rest locals; the lab's gait moves its feet's).
                    let drawn = length(sub(self.posed[b].1, self.posed[p].1));
                    let bind = length(self.rest_local(b));
                    self.stats.length_change = self.stats.length_change.max((drawn - bind).abs());
                }
            }
        }
        // A catch step's legs swing on the steps' own pose (a person's step, faster than the
        // bound): left out while the steps last.
        let stepping = self.rig.balance.is_some();
        if let (Some(c0), None) = (self.clean_before, self.rig.fall.as_ref()) {
            for b in 0..COUNT {
                if stepping && mq::role_side(b).0.is_leg() {
                    continue;
                }
                // (The steps' own move of the actor moves the animation, not the drawn body.)
                let moved = add(
                    sub(sub(self.posed[b].1, before[b].1), sub(clean[b].1, c0[b].1)),
                    self.own_moved,
                );
                let drawn = mul(&self.posed[b].0, &transpose(&before[b].0));
                let own = mul(&clean[b].0, &transpose(&c0[b].0));
                let turn = length(rotation_vector(&mul(&drawn, &transpose(&own))));
                if length(moved) > self.stats.drawn_move {
                    self.stats.drawn_move = length(moved);
                    self.stats.drawn_move_bone = b;
                    self.stats.drawn_move_at = self.t;
                }
                if turn.to_degrees() > self.stats.drawn_turn_step {
                    self.stats.drawn_turn_step = turn.to_degrees();
                    self.stats.drawn_turn_bone = b;
                }
            }
        }
        self.clean_before = Some(clean);
        // Smoothness: each bone's velocity, acceleration and jerk from its shown position; the
        // biggest change of velocity in a frame and the biggest jerk (m/s, m/s3).
        let v: [V3; COUNT] = std::array::from_fn(|b| {
            scale(
                sub(self.posed[b].1, before[b].1),
                1.0 / (dt * UNITS_PER_METRE),
            )
        });
        if let Some(v0) = self.velocity {
            let a: [V3; COUNT] = std::array::from_fn(|b| scale(sub(v[b], v0[b]), 1.0 / dt));
            for b in 0..COUNT {
                let jump = length(sub(v[b], v0[b]));
                self.stats.velocity_jump = self.stats.velocity_jump.max(jump);
            }
            if let Some(a0) = self.acceleration {
                for b in 0..COUNT {
                    let j = length(sub(a[b], a0[b])) / dt;
                    self.stats.jerk = self.stats.jerk.max(j);
                }
            }
            self.acceleration = Some(a);
        }
        self.velocity = Some(v);
        let anim = self.anim();
        // The hand-back and the heading ([`Stats::settle_rise`], `settle_jump`, `yaw_rate`).
        let p = mq::PELVIS;
        let yaw = active::heading_off(
            &mul(&self.posed[p].0, &transpose(&anim[p].0)),
            self.anim_facing(),
        );
        self.stats.yaw_off = self.stats.yaw_off.max(yaw.abs());
        if let Some((a0, yaw0)) = self.anim_pelvis {
            let d = active::yaw_between(yaw0, yaw);
            self.stats.yaw_rate = self.stats.yaw_rate.max(d / dt);
            // Frames handed back from their start (the first one moved at full weight).
            if self.rig.settle.holding() && self.was_holding {
                let off = length(sub(self.posed[p].1, anim[p].1));
                let off0 = length(sub(before[p].1, a0));
                self.stats.settle_rise = self.stats.settle_rise.max(off - off0);
                let own = [self.walk * dt, 0.0, 0.0];
                let moved = length(sub(sub(self.posed[p].1, before[p].1), own));
                self.stats.settle_jump = self.stats.settle_jump.max(moved);
            }
        }
        // The reaction's offset over the animation (the pelvis's; each bone's turn).
        let off = sub(self.posed[p].1, anim[p].1);
        self.stats.pelvis_most = self.stats.pelvis_most.max(length(off));
        // The floor (Patrick's 19-43-08 clip): the pelvis under its animation, the feet under
        // theirs and under the floor (their ankles over it as the animation stands), the
        // reference's height moved by the steps.
        if self.rig.fall.is_none() {
            self.stats.pelvis_drop = self.stats.pelvis_drop.max(-off[2]);
            if self.stats.steps > 0 {
                self.stats.step_pelvis_drop = self.stats.step_pelvis_drop.max(-off[2]);
            } else {
                self.stats.flinch_pelvis_drop = self.stats.flinch_pelvis_drop.max(-off[2]);
            }
            for f in [mq::L_FOOT, mq::R_FOOT] {
                self.stats.foot_drop = self.stats.foot_drop.max(anim[f].1[2] - self.posed[f].1[2]);
                if let Some(g) = world::ground_point(&self.world, self.posed[f].1) {
                    self.stats.under_floor = self.stats.under_floor.max(g[2] - self.posed[f].1[2]);
                }
            }
            if let Some(g) = world::ground_point(&self.world, self.posed[p].1) {
                self.stats.under_floor = self.stats.under_floor.max(g[2] - self.posed[p].1[2]);
            }
            self.stats.reference_dz = self
                .stats
                .reference_dz
                .max((self.pos[2] - self.stats.start[2]).abs());
        }
        if let Some((a0, _)) = self.anim_pelvis {
            let off0 = sub(before[p].1, a0);
            let own = std::mem::take(&mut self.own_moved);
            // The actor following the feet moves the animation, not the shown pelvis.
            let j = length(add(sub(off, off0), own));
            self.stats.pelvis_jump = self.stats.pelvis_jump.max(j);
        }
        let offsets: [active::Q; COUNT] = std::array::from_fn(|b| {
            active::qmul(
                active::qfrom_m(&self.posed[b].0),
                active::qconj(active::qfrom_m(&anim[b].0)),
            )
        });
        if let Some(o0) = self.offsets_before {
            for b in 0..COUNT {
                let w = length(active::qlog(active::qmul(offsets[b], active::qconj(o0[b])))) / dt;
                if w > self.stats.bone_spin {
                    self.stats.bone_spin = w;
                    self.stats.spin_bone = b;
                }
            }
        }
        self.offsets_before = Some(offsets);
        self.anim_pelvis = Some((anim[p].1, yaw));
        self.was_holding = self.rig.settle.holding();
        let holding = self.rig.balance.is_some()
            || self.rig.fall.is_some()
            || self
                .rig
                .brace
                .is_some_and(|b| b.hands.iter().flatten().any(|h| !h.released()));
        if self.rig.body.is_some() && !holding {
            let off = length(sub(self.posed[p].1, anim[p].1));
            let base = *self.release_off.get_or_insert(off);
            self.stats.release_rise = self.stats.release_rise.max(off - base);
        }
        let mut torso_now = 0.0f32;
        // The spine's top against its animation (the host's `drawn_turn` reads it), its twist
        // about the vertical, and the head's, each frame from the first hit.
        if let Some(h0) = self.stats.first_hit {
            let off = |b: usize| mul(&self.posed[b].0, &transpose(&anim[b].0));
            let s2 = off(mq::SPINE2);
            let turn = length(rotation_vector(&s2)).to_degrees();
            let yaw = active::heading_off(&s2, self.anim_facing()).to_degrees();
            let head = length(rotation_vector(&off(mq::HEAD))).to_degrees();
            self.stats
                .spine_trace
                .push(((self.t - h0) as f32, turn, yaw, head));
        }
        for b in 0..COUNT {
            if mq::role_side(b).0.is_arm() {
                let turn = mul(&transpose(&before[b].0), &self.posed[b].0);
                let w = length(rotation_vector(&turn)) / dt;
                // Only the arm's own turn: less its parent's.
                let parent_turn = mq::PARENT[b].map_or(0.0, |p| {
                    length(rotation_vector(&mul(
                        &transpose(&before[p].0),
                        &self.posed[p].0,
                    ))) / dt
                });
                let own = (w - parent_turn).max(0.0);
                self.stats.arm_spin = self.stats.arm_spin.max(own);
            }
            let off = length(rotation_vector(&mul(
                &transpose(&anim[b].0),
                &self.posed[b].0,
            )));
            self.stats.flinch = self.stats.flinch.max(off.to_degrees());
            if matches!(
                mq::role_side(b).0,
                Role::Pelvis | Role::Spine | Role::Neck | Role::Head
            ) {
                self.stats.torso_turn = self.stats.torso_turn.max(off.to_degrees());
                torso_now = torso_now.max(off.to_degrees());
            }
        }
        // The host's `drawn_turn` (the torso's most turn off its animation) from frame to frame.
        if let Some(t0) = self.torso_before {
            let j = (torso_now - t0).abs();
            self.stats.turn_jump = self.stats.turn_jump.max(j);
            // Yielding to the AI's walk, and once handed back.
            if self.rig.yield_at.is_some() || (self.rig.body.is_none() && self.stats.hits > 0) {
                self.stats.yield_turn_jump = self.stats.yield_turn_jump.max(j);
            }
        }
        self.torso_before = Some(torso_now);
        if self.armed {
            let h = mq::R_HAND;
            let off = length(rotation_vector(&mul(
                &transpose(&anim[h].0),
                &self.posed[h].0,
            )));
            self.stats.aim_drift = self.stats.aim_drift.max(off.to_degrees());
        }
    }

    /// Whether a knock-down would happen now (the host's `knockdown::can_knock_now`): not within
    /// [`KNOCK_COOLDOWN`] of the last nor [`UP_COOLDOWN`] of the last get-up.
    pub fn can_knock_now(&self) -> Result<(), String> {
        if let Some(at) = self.last_knock.filter(|at| self.t - at < KNOCK_COOLDOWN) {
            return Err(format!("knocked down {:.1} s ago", self.t - at));
        }
        if let Some(at) = self.last_up.filter(|at| self.t - at < UP_COOLDOWN) {
            return Err(format!("got up {:.1} s ago", self.t - at));
        }
        Ok(())
    }

    /// The support's carrying of the body (the host's): while it goes over, eased down but never
    /// under [`FALL_SUPPORT`] (the ragdoll takes it; a posed fall that sagged to nothing folded into
    /// what was behind it and jittered); after a fall that did not happen, eased back up over
    /// [`FALL_BACK`].
    fn support(&mut self) -> f32 {
        if let Some(f) = &self.rig.fall {
            return (1.0 - active::ease(f.t / f.lead.max(1e-3))).max(FALL_SUPPORT);
        }
        match self.rig.support_back {
            Some((from, since)) => {
                let k = active::ease((self.t - since) as f32 / FALL_BACK);
                if k >= 1.0 {
                    self.rig.support_back = None;
                }
                from + (1.0 - from) * k
            }
            None => 1.0,
        }
    }

    /// A fall that did not happen (the host's `active_ragdoll::fall_failed`): the posed body stands
    /// back up from it, its support and its pelvis offset eased back over [`FALL_BACK`], never in
    /// one frame.
    fn fall_failed(&mut self, why: &str) {
        let Some(f) = self.rig.fall.take() else {
            return;
        };
        let support = (1.0 - active::ease(f.t / f.lead.max(1e-3))).max(FALL_SUPPORT);
        self.rig.support_back = Some((support, self.t));
        self.rig.shift_back = Some((self.rig.shift, self.t, FALL_BACK));
        self.note(format!("the fall did not happen ({why}): eased back up"));
    }

    // --- The ragdoll (the host's `knockdown::knock_down`, the launch, the get-up). ---------------

    /// The body dropped into its ragdoll kneeling (a test's: Patrick's 08-21-49 clip, a launch
    /// that landed it on its feet and folded it onto its knees): the shins back along the floor,
    /// the thighs and the torso upright, the pelvis lowered by the shins' length; no launch.
    pub fn drop_kneeling(&mut self) {
        let anim = self.anim();
        let mut rots: [M3; COUNT] = std::array::from_fn(|b| anim[b].0);
        let back = rotation(scale(
            unit(cross(self.anim_facing(), [0.0, 0.0, 1.0])).unwrap_or([1.0, 0.0, 0.0]),
            std::f32::consts::FRAC_PI_2,
        ));
        for b in [mq::L_CALF, mq::R_CALF, mq::L_FOOT, mq::R_FOOT] {
            rots[b] = mul(&back, &rots[b]);
        }
        let shin = length(sub(anim[mq::L_FOOT].1, anim[mq::L_CALF].1));
        let pelvis = sub(anim[mq::PELVIS].1, [0.0, 0.0, shin]);
        self.posed = self.place(&rots, pelvis);
        self.knock_down(self.anim_facing(), 0.0, "dropped kneeling");
    }

    fn knock_down(&mut self, direction: V3, body_speed: f32, why: &str) {
        if let Err(why) = self.can_knock_now() {
            self.note(format!("duplicate guard: {why}, not knocked again"));
            self.fall_failed(&why);
            return;
        }
        let speed = launch_speed(body_speed);
        let launch = launch_direction(direction);
        let v = vec(scale(launch, speed / UNITS_PER_METRE));
        let mass = self.mass();
        let mut bodies: Vec<Option<RigidBodyHandle>> = vec![None; COUNT];
        for (b, slot) in bodies.iter_mut().enumerate() {
            if !self.has_bone(b) {
                continue;
            }
            let (rot, pos) = self.posed[b];
            // The ragdoll goes on with the active body's motion (no jump), the launch on top.
            let (bv, bw) = self.rig.body.as_ref().map_or(([0.0; 3], [0.0; 3]), |body| {
                (body.state[b].v, body.state[b].w)
            });
            let rb = RigidBodyBuilder::dynamic()
                .pose(Pose::from_parts(to_m(pos), quat_of(&rot)))
                .linvel(v + vec(bv))
                .angvel(vec(bw))
                .linear_damping(0.05)
                .angular_damping(if mq::role_side(b).0.is_arm() {
                    2.0
                } else {
                    0.6
                })
                .ccd_enabled(true)
                .build();
            let h = self.world.insert_body(rb);
            let (a, c, r) = mq::CAPSULE[b];
            let a = to_m(scale(sub(a, mq::JOINT[b]), self.scale));
            let c = to_m(scale(sub(c, mq::JOINT[b]), self.scale));
            let col =
                ColliderBuilder::capsule_from_endpoints(a, c, r * self.scale / UNITS_PER_METRE)
                    .mass(mass * mq::MASS_SHARE[b])
                    .friction(0.8)
                    .restitution(0.0)
                    .collision_groups(InteractionGroups::new(
                        Group::GROUP_2,
                        Group::GROUP_1,
                        InteractionTestMode::And,
                    ))
                    .build();
            self.world.insert_collider(col, Some(h));
            *slot = Some(h);
        }
        for (b, hb) in bodies.iter().enumerate() {
            let (Some(p), Some(hb)) = (mq::PARENT[b], *hb) else {
                continue;
            };
            let Some(hp) = bodies[p] else {
                continue;
            };
            let lim = mq::ragdoll_limits(b);
            // An elbow and a knee are one-way hinges: one axis (X, across the body), its limits
            // (Rapier's three-axis limits let a hard fall fold them past their stops).
            if matches!(mq::role_side(b).0, Role::Forearm | Role::Calf) {
                let hinge = RevoluteJointBuilder::new(Vec3::X)
                    .local_anchor1(to_m(self.rest_local(b)))
                    .local_anchor2(Vec3::ZERO)
                    .contacts_enabled(false)
                    .limits(lim[0])
                    .motor_velocity(0.0, 0.05)
                    .motor_max_force(2.0);
                self.world.insert_impulse_joint(hp, hb, hinge.build());
                continue;
            }
            let frame1 = Pose::from_translation(to_m(self.rest_local(b)));
            let mut joint = GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES)
                .local_frame1(frame1)
                .local_frame2(Pose::IDENTITY)
                .contacts_enabled(false)
                .limits(JointAxis::AngX, lim[0])
                .limits(JointAxis::AngY, lim[1])
                .limits(JointAxis::AngZ, lim[2]);
            // A little friction in the joint (a ragdoll's damping).
            for axis in [JointAxis::AngX, JointAxis::AngY, JointAxis::AngZ] {
                joint = joint
                    .motor_velocity(axis, 0.0, 0.05)
                    .motor_max_force(axis, 2.0);
            }
            self.world.insert_impulse_joint(hp, hb, joint.build());
        }
        self.rig = Rig::new(self.rig.limbs);
        self.velocity = None;
        self.anim_pelvis = None;
        self.offsets_before = None;
        self.torso_before = None;
        self.clean_before = None;
        self.lift_low = [0.0; 2];
        self.own_moved = [0.0; 3];
        self.acceleration = None;
        self.last_knock = Some(self.t);
        self.nudges = 0;
        self.nudge_at = self.t;
        self.stats.fell = true;
        self.stats.fall.get_or_insert_with(|| why.to_string());
        self.state = State::Ragdoll {
            bodies,
            since: self.t,
            still: 0,
            dir: unit([direction[0], direction[1], 0.0]).unwrap_or([0.0; 3]),
            braced: false,
        };
        self.note(format!(
            "knocked down ({why}): every bone +{speed:.0} units/s ({:.1} m/s) along ({:.2}, \
             {:.2}, {:.2})",
            speed / UNITS_PER_METRE,
            launch[0],
            launch[1],
            launch[2]
        ));
    }

    /// Whether the body still has `bone` (a lost limb takes its bones).
    pub fn has_bone(&self, bone: usize) -> bool {
        let l = self.rig.limbs;
        !match bone {
            mq::L_THIGH | mq::L_CALF | mq::L_FOOT => !l.left_leg,
            mq::R_THIGH | mq::R_CALF | mq::R_FOOT => !l.right_leg,
            mq::L_UPPERARM | mq::L_FOREARM | mq::L_HAND => !l.left_arm,
            mq::R_UPPERARM | mq::R_FOREARM | mq::R_HAND => !l.right_arm,
            _ => false,
        }
    }

    fn ragdoll_frame(&mut self) {
        // Before the step: the elbows and knees the last step left past straight are stopped.
        if let State::Ragdoll { bodies, .. } = &self.state {
            let handles = bodies.clone();
            self.stop_ragdoll_hinges(&handles);
        }
        self.brace_ground();
        self.world.step();
        let State::Ragdoll {
            ref bodies,
            since,
            ref mut still,
            ..
        } = self.state
        else {
            return;
        };
        let flying = self.t - since < FLIGHT;
        let arm_spin = self.tune.arm_ragdoll_spin;
        let mut ragdoll_arm_spin = self.stats.ragdoll_arm_spin;
        let mut fastest: f32 = 0.0;
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for (b, h) in bodies.iter().enumerate() {
            let Some(h) = h else {
                continue;
            };
            let Some(rb) = self.world.bodies.get_mut(*h) else {
                continue;
            };
            let v = from_m(rb.linvel());
            let arm = mq::role_side(b).0.is_arm();
            if flying || arm {
                // While it flies every bone is held; while it lies only an arm spinning past a
                // person's flung arm (the host's `hold_bones`).
                let l = length(v);
                if flying && l > MAX_BONE_SPEED {
                    rb.set_linvel(vec(scale(v, MAX_BONE_SPEED / l / UNITS_PER_METRE)), true);
                }
                let w = rb.angvel();
                // The arms spin no faster than a person's flung arm (the host's `hold_bones`).
                let most = if mq::role_side(b).0.is_arm() {
                    arm_spin.min(MAX_BONE_SPIN)
                } else {
                    MAX_BONE_SPIN
                };
                if w.length() > most {
                    rb.set_angvel(w * (most / w.length()), true);
                }
            }
            if mq::role_side(b).0.is_arm() {
                ragdoll_arm_spin = ragdoll_arm_spin.max(rb.angvel().length());
            }
            fastest = fastest.max(length(v));
            let pose = rb.position();
            let pos = from_m(pose.translation);
            self.posed[b] = (m3_of(pose.rotation), pos);
            low = low.min(pos[2]);
            high = high.max(pos[2]);
        }
        self.stats.ragdoll_arm_spin = ragdoll_arm_spin;
        if fastest < STILL_SPEED {
            *still += 1;
        } else {
            *still = 0;
        }
        let _ = (low, high, LYING_HEIGHT);
        // Lying: the pelvis near the floor and the torso near level (`knock::lying`, the host's
        // too); a still body that does not lie is nudged along its lean to finish toppling.
        let (pelvis, spine, head) = (
            self.posed[mq::PELVIS].1,
            self.posed[mq::SPINE1].1,
            self.posed[mq::HEAD].1,
        );
        let floor = world::ground_point(&self.world, [pelvis[0], pelvis[1], pelvis[2] + 40.0])
            .map_or(0.0, |g| g[2]);
        let lying = ragdoll_core::knock::lying(pelvis[2] - floor, spine, head, self.scale);
        let held = (self.t - since) as f32 >= HOLD_SECONDS;
        if *still >= STILL_FRAMES
            && !lying
            && !held
            && self.nudges < ragdoll_core::knock::NUDGES
            && (self.t - self.nudge_at) as f32 >= ragdoll_core::knock::NUDGE_EVERY
        {
            let facing = apply_m(&rotation([0.0, 0.0, self.heading]), FORWARD);
            let way = ragdoll_core::knock::lean(spine, head, facing);
            let v = vec(scale(way, ragdoll_core::knock::NUDGE_SPEED));
            for b in [mq::SPINE1, mq::SPINE2, mq::NECK, mq::HEAD] {
                if let Some(Some(h)) = bodies.get(b) {
                    if let Some(rb) = self.world.bodies.get_mut(*h) {
                        let now = rb.linvel();
                        rb.set_linvel(now + v, true);
                    }
                }
            }
            self.nudges += 1;
            self.nudge_at = self.t;
            *still = 0;
            self.stats.nudges += 1;
        }
        if (*still >= STILL_FRAMES && lying) || held {
            self.stats.got_up_lying = Some(lying);
            self.stats.got_up_pelvis = pelvis[2] - floor;
            let handles: Vec<RigidBodyHandle> = bodies.iter().flatten().copied().collect();
            let pelvis = self.posed[mq::PELVIS].1;
            let at = world::ground_point(&self.world, [pelvis[0], pelvis[1], pelvis[2] + 40.0])
                .unwrap_or([pelvis[0], pelvis[1], self.pos[2]]);
            let lay = (self.t - since) as f32;
            for h in handles {
                self.world.remove_body(h);
            }
            self.note(format!(
                "gets up after {lay:.2} s down{}: stands where the pelvis lies, {:.2} m from the \
                 start",
                if held {
                    " (held too long)"
                } else {
                    " (still and lying)"
                },
                length(sub(
                    [at[0], at[1], 0.0],
                    [self.stats.start[0], self.stats.start[1], 0.0]
                )) / UNITS_PER_METRE
            ));
            self.pos = at;
            self.last_up = Some(self.t);
            self.state = State::GettingUp {
                from: Box::new(self.posed),
                t: 0.0,
            };
        }
    }

    /// Stage 3's ground brace (the host's `knockdown::brace_ground`): in the ragdoll's first
    /// moments, while the pelvis is high over its lowest bone and dropping, the hands and forearms
    /// (the arms it has) are steered a share a frame toward a speed along the fall and down.
    fn brace_ground(&mut self) {
        let t = self.tune;
        let limbs = self.rig.limbs;
        let State::Ragdoll {
            ref bodies,
            since,
            dir,
            ref mut braced,
            ..
        } = self.state
        else {
            return;
        };
        if (self.t - since) as f32 >= t.ground_brace_for {
            return;
        }
        let Some(Some(pelvis)) = bodies.get(mq::PELVIS) else {
            return;
        };
        let low = bodies
            .iter()
            .flatten()
            .filter_map(|h| self.world.bodies.get(*h))
            .map(|b| b.translation().z * UNITS_PER_METRE)
            .fold(f32::INFINITY, f32::min);
        let Some(p) = self.world.bodies.get(*pelvis) else {
            return;
        };
        let dropping = p.linvel().z * UNITS_PER_METRE < -t.ground_brace_drop;
        if p.translation().z * UNITS_PER_METRE - low < t.ground_brace_height || !dropping {
            return;
        }
        let want = vec([
            dir[0] * t.ground_brace_along,
            dir[1] * t.ground_brace_along,
            -t.ground_brace_down,
        ]);
        let arms = [
            (mq::L_HAND, limbs.left_arm),
            (mq::L_FOREARM, limbs.left_arm),
            (mq::R_HAND, limbs.right_arm),
            (mq::R_FOREARM, limbs.right_arm),
        ];
        let mut moved = 0;
        for (bone, has) in arms {
            let Some(Some(h)) = bodies.get(bone).filter(|_| has) else {
                continue;
            };
            if let Some(b) = self.world.bodies.get_mut(*h) {
                let v = b.linvel();
                let mut nv = v + (want - v) * t.ground_brace_share;
                let max = MAX_BONE_SPEED / UNITS_PER_METRE;
                if nv.length() > max {
                    nv *= max / nv.length();
                }
                b.set_linvel(nv, true);
                moved += 1;
            }
        }
        if moved > 0 && !*braced {
            *braced = true;
            self.note(format!(
                "braces for the ground with its arms ({moved} arm bones)"
            ));
        }
    }

    fn get_up_frame(&mut self, dt: f32) {
        let State::GettingUp {
            ref from,
            ref mut t,
        } = self.state
        else {
            return;
        };
        *t += dt;
        let from = **from;
        let k = (*t / GET_UP).clamp(0.0, 1.0);
        let k = active::ease(k);
        let anim = self.anim();
        let was = self.posed;
        // Each bone's turn against its parent blended (not its world rotation: two bones blended
        // apart could take the joint between them the long way round, through a backward bend).
        let local = |pose: &[Xf; COUNT], b: usize| match mq::PARENT[b] {
            Some(p) => mul(&transpose(&pose[p].0), &pose[b].0),
            None => pose[b].0,
        };
        let mut world = [IDENTITY; COUNT];
        for b in 0..COUNT {
            let qa = quat_of(&local(&from, b));
            let qb = quat_of(&local(&anim, b));
            let l = m3_of(qa.slerp(qb, k));
            world[b] = match mq::PARENT[b] {
                Some(p) => mul(&world[p], &l),
                None => l,
            };
            let p = add(from[b].1, scale(sub(anim[b].1, from[b].1), k));
            self.posed[b] = (world[b], p);
        }
        // The get-up's fastest bone (units a second; Patrick's 08-21-49 clip: yanked flat in
        // four frames).
        if dt > 0.0 {
            for b in 0..COUNT {
                let v = length(sub(self.posed[b].1, was[b].1)) / dt;
                self.stats.get_up_fastest = self.stats.get_up_fastest.max(v);
            }
        }
        if k >= 1.0 {
            self.state = State::Standing;
            self.rig = Rig::new(self.rig.limbs);
            self.posed = anim;
            self.note("on its feet again".into());
        }
    }

    // --- The weapons. ----------------------------------------------------------------------------

    /// The weapon in hand fired at `point` on the body along `direction` (unit) from `range`
    /// metres: a gun's pellets spread over the body (`pick` finds what each pellet hits), a
    /// swing, or a blast centred on `point` (`point` then is the blast's centre).
    pub fn fire(&mut self, point: V3, direction: V3, part: &str, pick: &Pick) {
        match self.weapon.clone() {
            Weapon::Gun { gun, load } => {
                let Some((mut round, pellets)) = weapons::round(gun, load) else {
                    return;
                };
                // Hot Loads: the player's guns 10% faster at the muzzle.
                round.speed = perks::hot_loads(round.speed, self.perks.hot_loads);
                let radius = if pellets > 1 {
                    PELLET_SPREAD * self.range
                } else {
                    0.0
                };
                let across = unit(cross(direction, [0.0, 0.0, 1.0])).unwrap_or([1.0, 0.0, 0.0]);
                let up = unit(cross(across, direction)).unwrap_or([0.0, 0.0, 1.0]);
                let mut landed = 0;
                for i in 0..pellets {
                    let (dx, dy) = if i == 0 || radius <= 0.0 {
                        (0.0, 0.0)
                    } else {
                        let a = self.random() * std::f32::consts::TAU;
                        let r = radius * self.random().sqrt();
                        (r * a.cos(), r * a.sin())
                    };
                    let aim = add(point, add(scale(across, dx), scale(up, dy)));
                    let from = sub(aim, scale(direction, 300.0));
                    if let Some((bone, at)) = pick(self, from, direction) {
                        landed += 1;
                        let name = if pellets > 1 {
                            format!("{} pellet {}", mq::short_name(bone), i + 1)
                        } else {
                            format!("{} ({})", mq::short_name(bone), gun.name)
                        };
                        self.hit(Push::Round(round), at, direction, &name);
                    }
                }
                if pellets > 1 {
                    self.note(format!(
                        "{}: {landed} of {pellets} pellets landed ({:.0} m, spread {radius:.0} \
                         units)",
                        gun.name, self.range
                    ));
                }
            }
            Weapon::Melee {
                melee,
                strength,
                skill,
                power,
            } => {
                let dir = unit([direction[0], direction[1], 0.0]).unwrap_or(direction);
                let push = weapons::swing(melee, strength, skill, power);
                self.hit(push, point, dir, &format!("{part} ({})", melee.name));
            }
            Weapon::Blast { blast } => {
                let chest = add(self.pos, [0.0, 0.0, 60.0 * self.scale]);
                let d = sub(chest, point);
                let distance = length(d);
                let damage = weapons::blast_damage(blast, distance);
                let dir = unit(d).unwrap_or([0.0, 0.0, 1.0]);
                if damage <= 0.0 {
                    self.note(format!(
                        "{} {:.1} m away: out of its radius",
                        blast.name,
                        distance / UNITS_PER_METRE
                    ));
                    return;
                }
                self.hit(
                    Push::Blast { damage },
                    chest,
                    dir,
                    &format!(
                        "{} {:.1} m away ({damage:.0} damage)",
                        blast.name,
                        distance / UNITS_PER_METRE
                    ),
                );
            }
        }
    }
}

fn describe(push: Push) -> String {
    match push {
        Push::Round(r) => format!(
            "{} {:.1} g at {:.0} m/s ({:.2} N s, {:.0}% left in the body):",
            if r.pellet { "pellet" } else { "round" },
            r.mass * 1000.0,
            r.speed,
            r.momentum(),
            r.share * 100.0
        ),
        Push::Swing {
            weight,
            strength,
            power,
            ..
        } => format!(
            "swing ({weight:.0} lb, Strength {strength:.0}{}):",
            if power { ", power attack" } else { "" }
        ),
        Push::Blast { damage } => format!("blast ({damage:.0} damage here):"),
        Push::Damage { damage } => format!("hit of no known calibre ({damage:.0} damage):"),
    }
}

/// nv-rs's own perks the lab's player has (`ragdoll_core::perks`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Perks {
    pub stopping_power: bool,
    pub hot_loads: bool,
    pub bowling_ball: bool,
}
