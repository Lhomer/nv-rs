//! New Physics' balance: where the body can still be caught, and the steps that catch it.
//!
//! Each frame: the whole body's centre of mass and its velocity against the animation's (a walk's
//! own motion is not a push); the capture point (`com + v / ω₀`, `ω₀ = √(g / h)`: where the body
//! comes to rest over a point foot); the support (the hull of the planted feet's soles, each foot's
//! capsule with its toe and heel). Standing, a capture point inside the support is held by the
//! ankles and hips ([`Balance::holds`]); past it by [`MARGIN`], a step: the foot that reaches it
//! without crossing the other, landing [`STEP_PAST`] beyond it, at most [`STEP_MOST`] long, over
//! [`STEP_TIME`]; after landing it tests again; past [`MAX_STEPS`] steps still outside, it falls.
//! Moving (the animation walks or runs it), its capture point is held against the animation's own:
//! knocked off its path by more than [`GATHER`], a gather step puts the foot there and the path
//! takes the rest.
//!
//! The steps move the body against its animation: [`Balance::shift`] (the planted feet's middle
//! against the animated feet's, eased), which the controller adds to the animated pose and the
//! game turns into the actor's own move (`crate::np::Np::take_move`).
//!
//! Nothing here poses the body: it says where a foot should go and how much the root is held; the
//! physics does the rest.

use crate::active::{ease, Q};
use crate::flinch::{add, cross, dot, length, scale, sub, unit, V3};
use crate::np_body::{Model, State, GRAVITY};

/// How far past the support the capture point goes before a step (m).
pub const MARGIN: f32 = 0.02;
/// How far beyond the capture point a step lands (m), the longest step (m), and a step's swing
/// (s) and lift (m).
pub const STEP_PAST: f32 = 0.08;
pub const STEP_MOST: f32 = 0.65;
pub const STEP_TIME: f32 = 0.3;
pub const STEP_LIFT: f32 = 0.08;
/// Steps before it falls, and how far outside its feet the capture point may be before no step can
/// catch it (m: the longest step and the foot's reach past it): it falls at once.
pub const MAX_STEPS: u32 = 3;
pub const UNREACHABLE: f32 = 0.9;
/// How far a foot may stand off its animated place, once caught, before it steps back (m), and
/// that step's swing (s).
pub const STANCE_OFF: f32 = 0.1;
pub const STANCE_TIME: f32 = 0.45;
/// How long the capture point stays inside before it stands again (s).
pub const SETTLE: f32 = 0.3;
/// Standing again, a planted foot its animation moves faster than this (m/s, the animation's own
/// foot, the shift aside) is let go to go with it: a slow walk or turn under [`MOVING`] left the
/// feet where they had landed while the animation went on, the capture point drifted out and
/// the steps it took walked a 0.32 m/s hit into a fall (the testers' b507e997 logs: "stood", then
/// three catching steps with no hit, the hips lowered 0.15 m, out of steps).
pub const ANIM_FOOT_MOVING: f32 = 0.05;
/// Moving: how far its capture point may leave the animation's before a gather step (m), and the
/// animation's speed that counts as moving (m/s).
pub const GATHER: f32 = 0.12;
pub const MOVING: f32 = 0.3;
/// Moving: how fast its path follows its knocked body (per second), and how far its capture point
/// may be off its path (the walk's speed at the knock) before it falls (m).
pub const PATH_RATE: f32 = 6.0;
/// Moving: how long after a knock its path follows its body (s), and how far off the body may be
/// before it does (m: a walk's own sway is not a knock).
pub const PATH_FOLLOW: f32 = 1.0;
pub const PATH_DEAD: f32 = 0.03;
pub const PATH_FALL: f32 = 0.7;
/// How fast the shift follows the feet when no step swings (per second).
pub const SHIFT_RATE: f32 = 3.0;
/// After its swing a step's foot is pulled on down to its spot until it stands, at most this
/// long (s).
pub const LAND_MOST: f32 = 0.15;
/// After a step no other starts until the landed foot stands, at most this long (s).
pub const LAND_WAIT: f32 = 0.25;

/// What the balance is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The capture point inside the support (or moving on its path).
    Standing,
    /// A step swings.
    Stepping,
    /// Landed, the capture point not yet back inside for [`SETTLE`].
    Recovering,
    /// Out of steps with the capture point still outside: the ragdoll's.
    Falling,
}

/// A step under way.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    /// 0 the left foot, 1 the right.
    pub side: usize,
    /// The foot body's centre at lift-off and its landing (world, m; the landing at the same
    /// height over the floor it stood at).
    pub from: V3,
    pub to: V3,
    /// Its rotation kept through the swing.
    pub q: Q,
    pub t: f32,
    pub time: f32,
    pub lift: f32,
    /// A step back to the animated stance (not a catch: it does not count).
    pub stance: bool,
}

/// One actor's balance.
#[derive(Clone, Debug)]
pub struct Balance {
    pub phase: Phase,
    pub step: Option<Step>,
    pub steps: u32,
    /// Seconds in the phase.
    pub since: f32,
    /// The body's move against its animation (world, m, flat).
    pub shift: V3,
    /// The shift at a step's start and where it goes by its landing.
    shift_from: V3,
    shift_to: V3,
    /// The last frame's figures (for the log and the tests): the capture point, the support's
    /// points, how far the capture point is outside it (m; negative inside), whether moving.
    pub capture: V3,
    pub support: Vec<[f32; 2]>,
    pub outside: f32,
    pub moving: bool,
    /// The feet's bodies (left, right).
    pub feet: [usize; 2],
    /// Each leg's joints (hip, knee, ankle).
    pub legs: [Vec<usize>; 2],
    /// The steps taken since the hit (for the log), catches and stance steps.
    pub taken: Vec<(usize, V3, bool)>,
    /// Where each foot that stepped stands (its body's centre and rotation): held there (the
    /// leg's muscles pulled a landed foot back under the hip, toward the animation's stance) until
    /// a stance step brings it back.
    pub plants: [Option<(V3, Q)>; 2],
    /// The thighs' bodies (left, right: the hips' joints at their nodes) and each leg's length
    /// (hip to foot body, m).
    pub thighs: [usize; 2],
    pub leg_length: [f32; 2],
    /// The root held lower now (m, eased).
    pub lower: f32,
    /// Moving: seconds since it was knocked off its path (its path follows it meanwhile).
    pub knocked: Option<f32>,
    /// Why it fell: "steps" (out of steps), "unreachable" (past any step's reach at once),
    /// "path" (moving, knocked too far off its path); empty while it has not.
    pub fall_reason: &'static str,
    /// Whether a foot stood on the floor as the support was last measured.
    pub supported: bool,
    /// Each animated foot's place the last frame (m, the shift aside).
    pub anim_feet: [Option<V3>; 2],
    /// The floor's height the last frame (m).
    pub floor_was: Option<f32>,
    /// The last frame's centre of mass velocity against the animation's (m/s).
    pub rel_v: V3,
}

/// What the balance asks of the body this frame.
#[derive(Clone, Copy, Debug)]
pub struct Ask {
    /// Each leg's muscles' share (a swinging leg relaxed under its foot's pull).
    pub leg: [f32; 2],
    /// The swinging foot: its body, the centre and rotation it is pulled to, its velocity (m/s).
    pub foot: Option<(usize, V3, Q, V3)>,
    /// The planted feet held where they landed (body, centre, rotation).
    pub plants: [Option<(usize, V3, Q)>; 2],
    /// How much the root may be held across (the ankles' and hips' reach: strong while the capture
    /// point is inside, weak once it is out), 0 to 1.
    pub hold_across: f32,
    /// Falling: the muscles and the support let go.
    pub falling: bool,
    /// How much lower the root is held (m): spread feet need the hips lower to reach them.
    pub lower: f32,
}

impl Balance {
    pub fn new(model: &Model) -> Balance {
        let foot = |n: &str| model.body(n).unwrap_or(0);
        let leg = |side: &str| -> Vec<usize> {
            model
                .joints
                .iter()
                .enumerate()
                .filter(|(_, j)| {
                    let n = &model.bodies[j.child].node;
                    n.starts_with(&format!("Bip01 {side} "))
                        && (n.ends_with("Thigh") || n.ends_with("Calf") || n.ends_with("Foot"))
                })
                .map(|(i, _)| i)
                .collect()
        };
        Balance {
            phase: Phase::Standing,
            step: None,
            steps: 0,
            since: 0.0,
            shift: [0.0; 3],
            shift_from: [0.0; 3],
            shift_to: [0.0; 3],
            capture: [0.0; 3],
            support: Vec::new(),
            outside: -1.0,
            moving: false,
            feet: [foot("Bip01 L Foot"), foot("Bip01 R Foot")],
            legs: [leg("L"), leg("R")],
            taken: Vec::new(),
            plants: [None, None],
            thighs: [foot("Bip01 L Thigh"), foot("Bip01 R Thigh")],
            leg_length: {
                let (_, nodes) = model.bind();
                let st = model.states_from_nodes(&nodes);
                let f = [foot("Bip01 L Foot"), foot("Bip01 R Foot")];
                let t = [foot("Bip01 L Thigh"), foot("Bip01 R Thigh")];
                [0, 1].map(|i| {
                    length(sub(
                        st[f[i]].x,
                        scale(nodes[t[i]].1, 1.0 / crate::np_body::UNITS),
                    ))
                })
            },
            lower: 0.0,
            knocked: None,
            fall_reason: "",
            supported: true,
            anim_feet: [None, None],
            floor_was: None,
            rel_v: [0.0; 3],
        }
    }

    /// A foot's sole as points on the floor (toe and heel, each side): its capsule's ends pushed
    /// out by its radius along it and across (m, flat).
    fn sole(model: &Model, s: &State, b: usize) -> Vec<[f32; 2]> {
        let def = &model.bodies[b];
        let e: Vec<V3> = def
            .ends
            .iter()
            .map(|e| add(s.x, crate::active::qrot(s.q, sub(*e, def.com))))
            .collect();
        let along = unit([e[0][0] - e[1][0], e[0][1] - e[1][1], 0.0]).unwrap_or([0.0, 1.0, 0.0]);
        let across = [-along[1], along[0], 0.0];
        let r = def.radius;
        let mut out = Vec::new();
        for (p, k) in [(e[0], 1.0f32), (e[1], -1.0)] {
            for side in [-0.6f32, 0.6] {
                let q = add(add(p, scale(along, k * r)), scale(across, side * r));
                out.push([q[0], q[1]]);
            }
        }
        out
    }

    /// Whether a foot stands on the floor (its lowest point within 2 cm).
    fn planted(model: &Model, s: &State, b: usize, floor: f32) -> bool {
        let def = &model.bodies[b];
        def.ends.iter().any(|e| {
            add(s.x, crate::active::qrot(s.q, sub(*e, def.com)))[2] - def.radius < floor + 0.03
        })
    }

    /// The support (the planted feet's soles, the swinging one's not) and how far the capture
    /// point lies outside it.
    fn measure(&mut self, model: &Model, states: &[State], floor: f32) {
        let swinging = self.step.map(|s| s.side);
        let mut pts: Vec<[f32; 2]> = Vec::new();
        for side in 0..2 {
            let b = self.feet[side];
            if Some(side) != swinging && Self::planted(model, &states[b], b, floor) {
                pts.extend(Self::sole(model, &states[b], b));
            }
        }
        // No foot down (the feet a little off the floor, both swinging with the animation): the
        // feet's soles as they are, never "no support" on its own (Patrick's 803260d clip: a
        // floor that left the feet made the capture point 1 m outside, past
        // [`UNREACHABLE`], and he fell at 0.12 m/s).
        self.supported = !pts.is_empty();
        if pts.is_empty() {
            for side in 0..2 {
                pts.extend(Self::sole(model, &states[self.feet[side]], self.feet[side]));
            }
        }
        self.support = hull(&pts);
        self.outside = if self.support.is_empty() {
            1.0
        } else {
            outside(&self.support, [self.capture[0], self.capture[1]])
        };
    }

    /// The capture point's hold: true while it is inside the support.
    pub fn holds(&self) -> bool {
        self.outside <= MARGIN && self.phase != Phase::Falling
    }

    /// One frame: `states` the body, `anim` the animation's bodies (shifted by [`Balance::shift`]
    /// already), `anim_v` the animation's centre of mass velocity (m/s), the floor (m).
    pub fn update(
        &mut self,
        model: &Model,
        states: &[State],
        anim: &[State],
        anim_v: V3,
        floor: f32,
        dt: f32,
    ) -> Ask {
        self.since += dt;
        let (com, v) = model.centre_of_mass(states);
        let (anim_com, _) = model.centre_of_mass(anim);
        let height = (com[2] - floor).max(0.2);
        let omega = (GRAVITY / height).sqrt();
        let rel_v = sub(v, anim_v);
        self.rel_v = rel_v;
        let flat = |a: V3| [a[0], a[1], 0.0];
        self.capture = add(flat(com), scale(flat(rel_v), 1.0 / omega));
        self.moving = length(flat(anim_v)) > MOVING;
        // The footing moves with the floor: a step's heights and the plants' (a floor that rose
        // under a step held to the heights it was planned at, walking up a slope, pressed the
        // foot into it at 4 body weights).
        if let Some(was) = self.floor_was.replace(floor) {
            let dz = floor - was;
            if dz != 0.0 && dz.abs() < 0.5 {
                if let Some(st) = self.step.as_mut() {
                    st.from[2] += dz;
                    st.to[2] += dz;
                }
                for p in self.plants.iter_mut().flatten() {
                    p.0[2] += dz;
                }
            }
        }
        // Standing again: a planted foot the animation itself moves goes with it.
        for side in 0..2 {
            let at = sub(anim[self.feet[side]].x, self.shift);
            let moved = self.anim_feet[side].map_or(0.0, |was| length(sub(at, was)));
            self.anim_feet[side] = Some(at);
            if self.phase == Phase::Standing
                && self.since >= SETTLE
                && self.step.is_none()
                && dt > 0.0
                && moved / dt > ANIM_FOOT_MOVING
            {
                self.plants[side] = None;
            }
        }
        self.measure(model, states, floor);
        // The animation's own capture point (its centre over its feet, moving with it).
        let anim_capture = flat(anim_com);
        let off_path = sub(self.capture, anim_capture);
        let plants = |b: &Balance| {
            [0, 1].map(|side| {
                b.plants[side]
                    .filter(|_| b.step.map_or(true, |s| s.side != side))
                    .map(|(x, q)| (b.feet[side], x, q))
            })
        };
        // The hips lowered so each foot (where it stands or lands) is within its leg's reach,
        // beyond what the animation's own stance needs.
        let hip = |s: &[State], side: usize| {
            let b = self.thighs[side];
            let def = &model.bodies[b];
            sub(s[b].x, crate::active::qrot(s[b].q, def.com))
        };
        let mut need = 0.0f32;
        for side in 0..2 {
            let b = self.feet[side];
            let foot = match self.step.filter(|st| st.side == side) {
                Some(st) => st.to,
                None => states[b].x,
            };
            let reach = |h: V3, f: V3| {
                let l = self.leg_length[side];
                let d = length(flat(sub(f, h))).min(l * 0.95);
                l - (l * l - d * d).sqrt()
            };
            let now = reach(hip(states, side), foot);
            let anim_need = reach(hip(anim, side), anim[b].x);
            need = need.max(now - anim_need);
        }
        // Moving on its path the walk's own legs reach the floor.
        let need = if self.moving && self.step.is_none() {
            0.0
        } else {
            need
        };
        let k = 1.0 - (-8.0 * dt).exp();
        self.lower += (need.clamp(0.0, 0.15) - self.lower) * k;
        let mut ask = Ask {
            leg: [1.0, 1.0],
            foot: None,
            lower: self.lower,
            plants: plants(self),
            // Standing, the ankles and hips hold the body while the capture point is inside the
            // feet; moving, the gait carries it on its path while it stays on it.
            hold_across: if self.moving {
                if length(off_path) > GATHER {
                    0.4
                } else {
                    1.0
                }
            } else if self.outside > MARGIN {
                0.15
            } else {
                1.0
            },
            falling: false,
        };
        if self.phase == Phase::Falling {
            ask.falling = true;
            ask.hold_across = 0.0;
            return ask;
        }
        // A step under way.
        if let Some(mut st) = self.step {
            st.t += dt;
            let k = ease((st.t / st.time).min(1.0));
            self.shift = add(
                self.shift_from,
                scale(sub(self.shift_to, self.shift_from), k),
            );
            let b = self.feet[st.side];
            let down = Self::planted(model, &states[b], b, floor);
            if st.t >= st.time && (down || st.t >= st.time + LAND_MOST) {
                self.step = None;
                if !st.stance {
                    self.steps += 1;
                }
                // Held where it landed, at its standing height (pressed onto the floor).
                self.plants[st.side] =
                    Some(([states[b].x[0], states[b].x[1], st.to[2]], states[b].q));
                self.phase = Phase::Recovering;
                self.since = 0.0;
                ask.plants = plants(self);
                // The support again with the landed foot in it.
                self.measure(model, states, floor);
            } else if st.t >= st.time {
                // Landing: pressed down onto its spot.
                self.step = Some(st);
                ask.leg[st.side] = 0.25;
                ask.foot = Some((b, [st.to[0], st.to[1], st.to[2] - 0.04], st.q, [0.0; 3]));
                ask.hold_across = 0.6;
                return ask;
            } else {
                self.step = Some(st);
                let s = (st.t / st.time).clamp(0.0, 1.0);
                let along = ease(s);
                let mut at = add(st.from, scale(sub(st.to, st.from), along));
                at[2] += st.lift * (std::f32::consts::PI * s).sin();
                let dv = scale(sub(st.to, st.from), 1.0 / st.time);
                ask.leg[st.side] = 0.25;
                ask.foot = Some((self.feet[st.side], at, st.q, dv));
                ask.hold_across = 0.6;
                return ask;
            }
        }
        // No step swings: the shift follows the planted feet against the animated ones.
        let feet_mid =
            |s: &[State]| scale(add(flat(s[self.feet[0]].x), flat(s[self.feet[1]].x)), 0.5);
        if !self.moving {
            let want = add(self.shift, sub(feet_mid(states), feet_mid(anim)));
            let k = 1.0 - (-SHIFT_RATE * dt).exp();
            self.shift = add(self.shift, scale(sub(flat(want), self.shift), k));
        } else {
            // Moving: the knock moves its path (the shift follows the body's centre); the walk's
            // own steps gather it, the support's damper against the walk's velocity brings the run
            // back in line; knocked past what steps can take, it falls.
            if length(off_path) > GATHER {
                self.knocked = Some(0.0);
            }
            if let Some(t) = self.knocked.as_mut() {
                *t += dt;
                let off = flat(sub(com, anim_com));
                let l = length(off);
                if l > PATH_DEAD {
                    let k = 1.0 - (-PATH_RATE * dt).exp();
                    self.shift = add(self.shift, scale(off, k * (l - PATH_DEAD) / l));
                }
            }
            if self.knocked.is_some_and(|t| t > PATH_FOLLOW) {
                self.knocked = None;
            }
            if length(flat(rel_v)) / omega > PATH_FALL {
                self.fall_reason = "path";
                self.phase = Phase::Falling;
                self.since = 0.0;
                ask.falling = true;
                ask.hold_across = 0.0;
                return ask;
            }
            // Settled on its path as standing does: the steps start again (they were counted
            // across hits while the animation moved).
            self.settle();
            return ask;
        }
        // Just landed: no step until the landed foot stands (or a moment passes).
        let landing = self.phase == Phase::Recovering && self.since < LAND_WAIT && {
            let side = self.taken.last().map_or(0, |t| t.0);
            let b = self.feet[side];
            !Self::planted(model, &states[b], b, floor)
        };
        let out = !landing
            && if self.moving {
                length(off_path) > GATHER
            } else {
                self.outside > MARGIN
            };
        if out {
            if self.steps >= MAX_STEPS || self.outside > UNREACHABLE {
                self.fall_reason = if self.steps >= MAX_STEPS {
                    "steps"
                } else {
                    "unreachable"
                };
                self.phase = Phase::Falling;
                self.since = 0.0;
                ask.falling = true;
                ask.hold_across = 0.0;
                return ask;
            }
            if let Some(st) = self.plan(model, states, anim, floor) {
                self.begin(st, states, anim);
                ask.hold_across = 0.6;
            }
            return ask;
        }
        self.settle();
        match self.phase {
            Phase::Standing if self.since >= SETTLE && !self.moving => {
                // A foot back on its animated place lets go of its plant.
                for side in 0..2 {
                    let b = self.feet[side];
                    if length(flat(sub(states[b].x, anim[b].x))) < 0.03 {
                        self.plants[side] = None;
                    }
                }
                // Caught: the feet's stance against the animation's (each foot off its animated
                // place by the same move, which the shift then takes) is restored by one slow, low
                // step of the foot off by more: it lands off by what the other foot is, so the
                // shift that follows puts both on the animation's stance.
                let off = |side: usize| {
                    let b = self.feet[side];
                    flat(sub(states[b].x, anim[b].x))
                };
                let apart = length(sub(off(0), off(1)));
                if apart > STANCE_OFF {
                    let side = if length(off(0)) > length(off(1)) {
                        0
                    } else {
                        1
                    };
                    let b = self.feet[side];
                    let at = add(flat(anim[b].x), off(1 - side));
                    let to = [at[0], at[1], states[b].x[2]];
                    let st = Step {
                        side,
                        from: states[b].x,
                        to,
                        q: anim[b].q,
                        t: 0.0,
                        time: STANCE_TIME,
                        lift: STEP_LIFT * 0.5,
                        stance: true,
                    };
                    self.begin(st, states, anim);
                }
            }
            _ => {}
        }
        ask
    }

    /// The catching step: the foot that reaches past the capture point without crossing the other
    /// (it keeps its side of the body), the shorter of the two.
    fn plan(&self, model: &Model, states: &[State], anim: &[State], floor: f32) -> Option<Step> {
        let flat = |a: V3| [a[0], a[1], 0.0];
        let mid = scale(
            add(flat(states[self.feet[0]].x), flat(states[self.feet[1]].x)),
            0.5,
        );
        let (target_base, dir) = if self.moving {
            // Off its path: the gather step lands where the path's capture point lies plus the
            // knock (the animated feet's middle plus the capture point's offset).
            let (anim_com, _) = model.centre_of_mass(anim);
            let off = sub(self.capture, flat(anim_com));
            (add(self.capture, [0.0; 3]), unit(off)?)
        } else {
            (self.capture, unit(sub(self.capture, mid))?)
        };
        let mut best: Option<(f32, Step)> = None;
        for side in 0..2 {
            let b = self.feet[side];
            let foot = flat(states[b].x);
            // Its place across the push kept (no crossing), along it at the capture point.
            let rel = sub(foot, mid);
            let across = sub(rel, scale(dir, dot(rel, dir)));
            let mut to = add(add(target_base, scale(across, 0.8)), scale(dir, STEP_PAST));
            let mut d = sub(to, foot);
            let l = length(d);
            if l > STEP_MOST {
                d = scale(d, STEP_MOST / l);
                to = add(foot, d);
            }
            let height = states[b].x[2] - lowest_of(model, states, b) + floor;
            let st = Step {
                side,
                from: states[b].x,
                to: [to[0], to[1], height],
                q: states[b].q,
                t: 0.0,
                time: STEP_TIME,
                lift: STEP_LIFT,
                stance: false,
            };
            if best.as_ref().map_or(true, |(bl, _)| length(d) < *bl) {
                best = Some((length(d), st));
            }
        }
        let _ = cross;
        best.map(|b| b.1)
    }

    fn begin(&mut self, st: Step, states: &[State], anim: &[State]) {
        let flat = |a: V3| [a[0], a[1], 0.0];
        let other = self.feet[1 - st.side];
        // The shift by its landing: the feet's middle then against the animated feet's middle.
        let mid_then = scale(add(flat(st.to), flat(states[other].x)), 0.5);
        let anim_mid = scale(
            add(flat(anim[self.feet[0]].x), flat(anim[self.feet[1]].x)),
            0.5,
        );
        self.shift_from = self.shift;
        self.shift_to = add(self.shift, sub(mid_then, anim_mid));
        self.taken.push((st.side, st.to, st.stance));
        self.step = Some(st);
        self.phase = Phase::Stepping;
        self.since = 0.0;
    }

    /// The shift handed to the actor (it moves by it; the animation comes moved): taken off.
    /// Recovered for [`SETTLE`]: standing; standing for [`SETTLE`], moving or not: its catching
    /// steps start again.
    fn settle(&mut self) {
        match self.phase {
            Phase::Recovering if self.since >= SETTLE => {
                self.phase = Phase::Standing;
                self.since = 0.0;
            }
            Phase::Standing if self.since >= SETTLE => self.steps = 0,
            _ => {}
        }
    }

    pub fn take(&mut self, by: V3) {
        self.shift = sub(self.shift, by);
        self.shift_from = sub(self.shift_from, by);
        self.shift_to = sub(self.shift_to, by);
        // The actor moved: the animation comes moved by it, not by its own feet.
        for f in self.anim_feet.iter_mut().flatten() {
            *f = add(*f, by);
        }
    }
}

/// The lowest point of body `b` (m).
fn lowest_of(model: &Model, states: &[State], b: usize) -> f32 {
    let def = &model.bodies[b];
    def.ends
        .iter()
        .map(|e| {
            add(
                states[b].x,
                crate::active::qrot(states[b].q, sub(*e, def.com)),
            )[2] - def.radius
        })
        .fold(f32::INFINITY, f32::min)
}

/// The convex hull of points (Andrew's monotone chain), counter-clockwise.
pub fn hull(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut p: Vec<[f32; 2]> = points.to_vec();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let turn = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut lower: Vec<[f32; 2]> = Vec::new();
    for &q in &p {
        while lower.len() >= 2 && turn(lower[lower.len() - 2], lower[lower.len() - 1], q) <= 0.0 {
            lower.pop();
        }
        lower.push(q);
    }
    let mut upper: Vec<[f32; 2]> = Vec::new();
    for &q in p.iter().rev() {
        while upper.len() >= 2 && turn(upper[upper.len() - 2], upper[upper.len() - 1], q) <= 0.0 {
            upper.pop();
        }
        upper.push(q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// How far `p` lies outside the convex polygon `poly` (counter-clockwise; m): negative inside (the
/// distance to the nearest edge), positive outside.
pub fn outside(poly: &[[f32; 2]], p: [f32; 2]) -> f32 {
    let seg = |a: [f32; 2], b: [f32; 2]| {
        let d = [b[0] - a[0], b[1] - a[1]];
        let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
        let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
        let q = [a[0] + d[0] * t, a[1] + d[1] * t];
        ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
    };
    match poly.len() {
        0 => f32::INFINITY,
        1 => ((p[0] - poly[0][0]).powi(2) + (p[1] - poly[0][1]).powi(2)).sqrt(),
        2 => seg(poly[0], poly[1]),
        n => {
            let mut inside = true;
            let mut nearest = f32::INFINITY;
            for i in 0..n {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
                if cross < 0.0 {
                    inside = false;
                }
                nearest = nearest.min(seg(a, b));
            }
            if inside {
                -nearest
            } else {
                nearest
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_and_outside() {
        let h = hull(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.5, 0.5]]);
        assert_eq!(h.len(), 4);
        assert!((outside(&h, [0.5, 0.5]) + 0.5).abs() < 1e-5);
        assert!((outside(&h, [2.0, 0.5]) - 1.0).abs() < 1e-5);
        assert!((outside(&[[0.0, 0.0], [1.0, 0.0]], [0.5, 0.2]) - 0.2).abs() < 1e-5);
    }

    /// FNV's body in its bind pose, the floor under its lowest point (m).
    fn standing() -> (Model, Vec<State>, f32) {
        let sk = crate::np_skeleton::Skeleton::fnv();
        let m = Model::from_skeleton(&sk);
        let w = sk.bind_worlds();
        let nodes: Vec<(crate::flinch::M3, V3)> = m
            .bodies
            .iter()
            .map(|b| {
                let x = w[sk.node(&b.node).unwrap()];
                (x.r, x.t)
            })
            .collect();
        let s = m.states_from_nodes(&nodes);
        let floor = crate::np_body::lowest_point(&m, &s);
        (m, s, floor)
    }

    /// Each hit's catching steps are its own (Patrick's 23:41 clip: the AI walked, the steps added
    /// up across hits to 13 and a 0.69 m/s hit fell "steps"): a balance standing again past
    /// [`SETTLE`] starts its steps again while its animation moves too (5402dadc only while it
    /// stood still).
    #[test]
    fn the_steps_start_again_once_settled_while_moving() {
        let (m, s, floor) = standing();
        let mut b = Balance::new(&m);
        b.phase = Phase::Standing;
        b.since = SETTLE + 0.1;
        b.steps = 2;
        let moving = [MOVING * 1.5, 0.0, 0.0];
        let s_moving: Vec<State> = s.iter().map(|x| State { v: moving, ..*x }).collect();
        b.update(&m, &s_moving, &s, moving, floor, 1.0 / 60.0);
        assert!(b.moving);
        assert_eq!(b.steps, 0, "the steps were kept while moving");
    }

    /// A new hit's catching steps count from none.
    #[test]
    fn a_hit_starts_its_steps_from_none() {
        let sk = crate::np_skeleton::Skeleton::fnv();
        let m = Model::from_skeleton(&sk);
        let w = sk.bind_worlds();
        let nodes: Vec<(crate::flinch::M3, V3)> = m
            .bodies
            .iter()
            .map(|b| {
                let x = w[sk.node(&b.node).unwrap()];
                (x.r, x.t)
            })
            .collect();
        let mut np = crate::np::Np::new(m, &nodes, 1.0);
        np.balance.steps = 2;
        np.hit(0, nodes[0].1, [0.0, -20.0, 0.0]);
        assert_eq!(np.balance.steps, 0);
    }
}
