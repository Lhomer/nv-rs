//! New Physics' controller for one actor: its body ([`crate::np_body`], FNV's ragdoll) driven
//! toward the animation by its muscles ([`crate::np_drive`]) and held up at its root; a hit is an
//! impulse on the struck body and a softening of its chain; each frame answers the pose to draw
//! (every body's node's world). The host and the ragdoll lab run this same code.
//!
//! Milestone M1 holds the root up by a capped spring toward the animated root
//! ([`ROOT_SUPPORT`]); M2's balance replaces it with the feet's support, the capture point and
//! steps.

use crate::active::{qconj, qfrom_m, qmul, qnorm, Q};
use crate::flinch::{scale, M3, V3};
use crate::np_body::{Drive, Model, State, StepReport, Support, Tracking, UNITS};
use crate::np_drive::Muscles;

/// The root's support (M1): spring (N/m) and damper (N·s/m), turn spring (N·m/rad) and damper,
/// at most this force (N) and torque (N·m); a hit softens it by its share at the root.
pub const ROOT_SUPPORT: Support = Support {
    x: [0.0; 3],
    q: [1.0, 0.0, 0.0, 0.0],
    kp: 30000.0,
    kd: 2500.0,
    kp_turn: 8000.0,
    kd_turn: 120.0,
    max_force: 2000.0,
    max_torque: 600.0,
};

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
        Np {
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
    /// the floor at `floor` (game units); answers each body's node's world to draw.
    pub fn frame(&mut self, anim: &[(M3, V3)], floor: f32, dt: f32) -> Vec<(M3, V3)> {
        let anim_states = self.model.states_from_nodes(anim);
        let r = self.model.root;
        let jumped =
            crate::flinch::length(crate::flinch::sub(anim_states[r].x, self.anim_states[r].x))
                * UNITS
                > JUMP;
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
        }
        self.t += dt;
        if let Some(s) = self.since_hit.as_mut() {
            *s += dt;
        }
        self.muscles.advance(dt);
        let targets = relative_of(&self.model, anim);
        let drives: Vec<Drive> = targets
            .iter()
            .zip(self.muscles.gains())
            .map(|(&target, (kp, kd))| (target, kp, kd))
            .map(|(target, kp, kd)| Drive {
                target,
                kp,
                kd,
                hold: 1.0,
            })
            .collect();
        let soft = 1.0 - 0.5 * self.muscles.root_softening();
        let support = Support {
            x: self.anim_states[r].x,
            q: self.anim_states[r].q,
            kp: ROOT_SUPPORT.kp * soft,
            kd: ROOT_SUPPORT.kd * soft,
            kp_turn: ROOT_SUPPORT.kp_turn * soft,
            kd_turn: self.root_kd_turn * soft,
            ..ROOT_SUPPORT
        };
        // Tracking softened with the joint each body hangs from (the root with the support).
        let soft = self.muscles.softening();
        let mut rate = vec![
            TRACKING * (1.0 - TRACKING_SOFT * self.muscles.root_softening());
            self.states.len()
        ];
        for (j, def) in self.model.joints.iter().enumerate() {
            rate[def.child] = TRACKING * (1.0 - TRACKING_SOFT * soft[j]);
        }
        let tracking = Tracking { w: anim_w, rate };
        self.report = self.model.step_tracked(
            &mut self.states,
            &drives,
            Some(&support),
            Some(&tracking),
            floor / UNITS,
            dt,
        );
        self.model.node_worlds(&self.states)
    }
}
