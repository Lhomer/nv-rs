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
use crate::flinch::{add, scale, sub, M3, V3};
use crate::np_balance::{Balance, Phase};
use crate::np_body::{Drive, Model, State, StepReport, Support, Tracking, UNITS};
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
    anim_v: V3,
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
        let states = model.states_from_nodes(anim);
        let muscles = Muscles::new(&model, strength);
        let root_kd_turn = ROOT_SUPPORT
            .kd_turn
            .max(2.0 * (ROOT_SUPPORT.kp_turn * model.inertia_about_root()).sqrt());
        let balance = Balance::new(&model);
        Np {
            balance,
            anim_com: None,
            anim_v: [0.0; 3],
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
        self.model
            .impulse(&mut self.states, body, scale(at, 1.0 / UNITS), impulse);
        let size = crate::flinch::length(impulse);
        self.muscles.hit(&self.model, body, size);
        self.since_hit = Some(0.0);
    }

    /// One frame of `dt`: the body driven toward `anim` (each body's node's world, game units),
    /// moved by the balance's shift, the floor at `floor` (game units); answers each body's node's
    /// world to draw.
    pub fn frame(&mut self, anim: &[(M3, V3)], floor: f32, dt: f32) -> Vec<(M3, V3)> {
        let unshifted = self.model.states_from_nodes(anim);
        let (com_now, _) = self.model.centre_of_mass(&unshifted);
        let anim_v = match (self.anim_com, dt > 0.0) {
            (Some(was), true) => scale(sub(com_now, was), 1.0 / dt),
            _ => self.anim_v,
        };
        self.anim_v = anim_v;
        let jumped = self
            .anim_com
            .is_some_and(|was| crate::flinch::length(sub(com_now, was)) * UNITS > JUMP);
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
        if jumped {
            self.states = self.anim_states.clone();
            self.balance = Balance::new(&self.model);
        }
        self.t += dt;
        if let Some(s) = self.since_hit.as_mut() {
            *s += dt;
        }
        self.muscles.advance(dt);
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
        let tracking = Tracking {
            w: anim_w,
            rate,
            slip,
        };
        self.report = self.model.step_tracked(
            &mut self.states,
            &drives,
            &pulls,
            Some(&tracking),
            floor / UNITS,
            dt,
        );
        self.model.node_worlds(&self.states)
    }

    /// The animated pose (`anim`, shifted, game units) with its legs bent onto the balance's
    /// footing; none while the balance is not engaged (the animation's own legs).
    fn legs_on_footing(
        &self,
        anim: &[(M3, V3)],
        ask: &crate::np_balance::Ask,
    ) -> Option<Vec<(M3, V3)>> {
        let b = &self.balance;
        let engaged = if b.moving {
            // On its path the walk's legs drive; a gather step or a plant takes them.
            b.step.is_some() || b.plants.iter().any(|p| p.is_some())
        } else {
            b.phase != Phase::Standing || b.plants.iter().any(|p| p.is_some()) || b.lower > 0.003
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

    /// The animation's velocity (game units a second) before its first frame (an actor already
    /// walking when it is taken): the bodies move with it.
    pub fn set_anim_velocity(&mut self, v: V3) {
        self.anim_v = scale(v, 1.0 / UNITS);
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
        if self.balance.step.is_some() || self.balance.phase == Phase::Falling {
            return [0.0; 3];
        }
        let by = self.balance.shift;
        if crate::flinch::length(by) < 1e-4 {
            return [0.0; 3];
        }
        self.balance.take(by);
        if let Some(c) = self.anim_com.as_mut() {
            *c = add(*c, by);
        }
        scale(by, UNITS)
    }
}
