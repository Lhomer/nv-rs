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
/// How long the limits stay soft after a fall (s).
pub const LIMITS_SETTLE: f32 = 0.3;
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
pub const STAND_STILL: f32 = 0.4;
/// While the feet lie further than this from the hips (m, flat) the rise goes toward them.
pub const RISE_FEET_OUT: f32 = 0.15;
/// Rising from the floor, each foot's goal lies this share of its leg in front of its hip (a hip
/// on the floor; under it as the hip comes up a leg high).
pub const RISE_FOOT_OUT: f32 = 0.3;
/// The legs push once the centre of mass is within this of the feet (m): their pull brings the
/// hips forward over the feet as they lift.
pub const RISE_OVER_FEET: f32 = 0.35;
/// The legs' push lifts the pelvis at most this fast (m/s), slowing as it nears standing height
/// (its pace per metre still to go, 1/s).
pub const RISE_PACE: (f32, f32) = (0.8, 3.0);
/// The legs' push held through a lapse this long (s).
pub const LIFT_LAPSE: f32 = 0.2;

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
    /// Each body's own floor (world, metres: the ground under it, a slope's; empty: the frame's
    /// flat floor).
    pub floors: Vec<Plane>,
    /// The arm that holds a weapon (0 left, 1 right; none unarmed): it keeps it, and its aim
    /// while the AI fires from the ground; the free arm pushes.
    pub weapon_side: Option<usize>,
    /// The AI fires from the ground: the weapon arm aims, the free arm braces.
    pub firing: bool,
    /// About to be handed to the game's ragdoll: the body is brought inside the skeleton's own
    /// limits (`np_body::Tracking::data_limits`); `report.limit_most` says how far out it still is.
    pub to_limits: bool,
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
            body_walls: Vec::new(),
            weapon_side: None,
            firing: false,
            to_limits: false,
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
        let out = self.frame_local(&local, floor, dt);
        self.walls = walls;
        self.floors = floors;
        self.body_walls = body_walls;
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
            });
            return self.down_frame(anim, floor, dt);
        }
        let posed = self.legs_on_footing(&shifted, &ask);
        let targets = relative_of(&self.model, posed.as_deref().unwrap_or(anim));
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
                q: self.anim_states[r].q,
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
            soft_limits: self.stood_up < LIMITS_SETTLE || self.to_limits,
            floors: self.floors.clone(),
            data_limits: self.to_limits,
            body_walls: self.body_walls.clone(),
        };
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
        self.model.node_worlds(&self.states)
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
        }
        if rising && !airborne {
            if reading.pelvis_height > down.rise_best + RISE_GAINED {
                down.rise_best = reading.pelvis_height;
                down.rise_stall = 0.0;
            } else {
                down.rise_stall += dt;
            }
            let worlds = self.model.node_worlds(&self.states);
            let feet_mid = scale(add(self.states[b.feet[0]].x, self.states[b.feet[1]].x), 0.5);
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
            // The torso over the feet: up, leaning toward them by how far behind it is.
            let lean = (crate::flinch::length(flat) * RISE_LEAN.0).min(RISE_LEAN.1);
            let dir = crate::flinch::unit(flat).unwrap_or(rise_dir);
            let want_up = add(scale([0.0, 0.0, 1.0], lean.cos()), scale(dir, lean.sin()));
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
                targets[j] = turn_toward(now, qnorm(qmul(qconj(qp), goal)), RISE_REACH);
                gains[j].0 = self.muscles.base[j].0 * RISE_GAIN * k;
                holds[j] = 0.5 * k;
            }
            // Each foot drawn onto the floor in front of its hip, as near as the leg folds (half
            // its length out from a hip near the floor, under it once the hip is a leg high).
            let mut posed = worlds.clone();
            for side in 0..2 {
                let (thigh, calf, foot) = (b.thighs[side], b.calves[side], b.feet[side]);
                let hip = worlds[thigh].1;
                let leg = self.balance.leg_length[side] * UNITS;
                let foot_h = (anim[foot].1[2] - anim[r].1[2] + b.stand_height * UNITS).max(5.0);
                // This foot's own ground under its hip (a slope's), game units.
                let ground = self.ground_at(foot, hip).map_or(floor, |z| z * UNITS);
                let up = ((hip[2] - ground - foot_h) / leg).clamp(0.0, 1.0);
                let out = leg * (RISE_FOOT_OUT * (1.0 - up) + 0.05 * up);
                let ahead = scale(rise_dir, out);
                let at = [hip[0] + ahead[0], hip[1] + ahead[1], 0.0];
                let ground = self.ground_at(foot, at).map_or(floor, |z| z * UNITS);
                let goal = [at[0], at[1], ground + foot_h];
                let Some((r1, m, _)) = crate::flinch::leg_ik(
                    worlds[thigh].1,
                    worlds[calf].1,
                    worlds[foot].1,
                    hip,
                    goal,
                    rise_dir,
                ) else {
                    continue;
                };
                let mr1 = crate::flinch::mul(&m, &r1);
                posed[thigh].0 = crate::flinch::mul(&r1, &worlds[thigh].0);
                posed[calf].0 = crate::flinch::mul(&mr1, &worlds[calf].0);
                posed[foot].0 = anim[foot].0;
                // The foot drawn in along the floor (the leg's muscles against the floor's
                // grip: a heel dragged, not a leg swung up); once the legs push, the feet stand
                // where they are.
                if down.lifting_was {
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
            down.arms_off |= down.lifting_was;
            let arms = if down.arms_off {
                Vec::new()
            } else {
                self.pushing_arms(&worlds, floor)
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
            if over || (down.lifting_was && down.lift_lapse < LIFT_LAPSE) {
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
                let pace = ((stand_z - self.states[r].x[2]) * RISE_PACE.1).clamp(0.0, RISE_PACE.0);
                pulls.push(Support {
                    body: r,
                    x: [feet_mid[0], feet_mid[1], stand_z],
                    v: [0.0, 0.0, pace],
                    q: self.anim_states[r].q,
                    kp: ROOT_SUPPORT.kp,
                    kd: ROOT_SUPPORT.kd,
                    max_force: push,
                    // Held over the feet harder as it comes up: near upright the sway is the
                    // stand's to stop (it swayed 0.36 to 0.56 m/s upright and never stood:
                    // Patrick's 10:10 clip).
                    kp_h: ROOT_SUPPORT.kp_h * (0.2 + 0.8 * up_now),
                    kd_h: ROOT_SUPPORT.kd_h * (0.2 + 0.8 * up_now),
                    max_force_h: (250.0 + 650.0 * up_now) * k,
                    kp_turn: ROOT_SUPPORT.kp_turn * 0.5,
                    kd_turn: self.root_kd_turn * 0.5,
                    max_torque: 300.0 * k,
                });
            }
            // Not getting up (the pelvis no higher for a while): give up and settle back (the
            // AI may try again).
            if down.rise_stall > crate::np_down::RISE_STALL {
                self.rise = false;
                down.rise_failed = true;
                down.rising = 0.0;
                down.rise_best = 0.0;
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
            down.arms_off = false;
        } else if !rising_wish {
            down.rising = 0.0;
            down.rise_best = 0.0;
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
        // ragdoll after its tries).
        if self.rise && down.rising > RISE_MOST {
            self.rise = false;
            down.rise_failed = true;
            down.rising = 0.0;
            down.rise_best = 0.0;
            down.arms_off = false;
            down.rise_stall = 0.0;
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
            floors: self.floors.clone(),
            data_limits: self.to_limits,
            body_walls: self.body_walls.clone(),
        };
        self.report = self.model.step_world(
            &mut self.states,
            &drives,
            &pulls,
            Some(&tracking),
            f,
            &self.walls,
            dt,
        );
        // Standing again, balanced over its feet: the balance has it.
        let mut now = read(&self.model, &b, &self.states, &self.report, f);
        if down.lifting {
            now.pushed = (now.weight - now.total).max(0.0);
        }
        if classify(&now, false) == Label::Standing
            && crate::flinch::length(sub(now.com_v, self.ground_v)) < STAND_STILL
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
            let shift = self.balance.shift;
            self.balance = Balance::new(&self.model);
            self.balance.shift = shift;
            self.rise = false;
            self.down = None;
            self.stood_up = 0.0;
        } else {
            self.down = Some(down);
        }
        self.model.node_worlds(&self.states)
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
        if shoulder[2] - ground <= reach * 1.05 {
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
        let engaged = into_ground
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
        let by = self.balance.shift;
        if crate::flinch::length(by) < 1e-4 {
            return [0.0; 3];
        }
        self.balance.take(by);
        self.anim_moved(by);
        scale(by, UNITS)
    }
}
