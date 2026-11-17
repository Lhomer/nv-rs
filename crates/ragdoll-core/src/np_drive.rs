//! New Physics' muscles: each joint's spring and damper toward the animation (the design's
//! table), scaled by the actor's strength, softened along the struck chain by a hit and eased
//! back.
//!
//! A hit softens the joint of the struck body most and its neighbours less ([`SPREAD`] for each
//! joint further along the body), by up to [`SOFT_MOST`] for a hit of [`SOFT_FULL`] or more,
//! held [`SOFT_HOLD`] and eased back over [`RECOVER`]: the body gives under the hit and the
//! animation takes it back within 0.5 to 0.7 s.

use crate::active::ease;
use crate::np_body::Model;

/// The springs (N·m/rad) and dampers (N·m·s/rad) by the child body's node (the design's table).
pub const GAINS: [(&str, f32, f32); 9] = [
    ("Thigh", 500.0, 50.0),
    ("Calf", 400.0, 40.0),
    ("Foot", 200.0, 20.0),
    ("Spine", 600.0, 60.0),
    ("Neck1", 80.0, 8.0),
    ("Head", 40.0, 4.0),
    ("UpperArm", 120.0, 12.0),
    ("Forearm", 80.0, 8.0),
    ("Hand", 30.0, 3.0),
];

/// The most a hit softens a joint (a share of its spring), and the hit (N·s) that softens it
/// that much: 2 m/s on an 81 kg body.
pub const SOFT_MOST: f32 = 0.8;
pub const SOFT_FULL: f32 = 162.0;
/// How much less each joint further from the struck body is softened (a factor per joint).
pub const SPREAD: f32 = 0.6;
/// How long the softening holds (s) and how long it takes to ease back (s).
pub const SOFT_HOLD: f32 = 0.05;
pub const RECOVER: f32 = 0.55;

/// The dampers' share of critical damping for the inertia each joint moves.
pub const DAMPING: f32 = 1.0;

/// The inertia (kg·m²) each joint turns: its subtree's bodies about the joint's pivot (their
/// masses at their distances and their own inertia, a scalar), in the model's bind pose.
pub fn joint_inertia(model: &Model) -> Vec<f32> {
    model.below_inertia.clone()
}

/// A joint's base spring and damper by its child body's node name.
pub fn base_gains(child: &str) -> (f32, f32) {
    let name = child.trim_start_matches("Bip01 ");
    let name = name.trim_start_matches(['L', 'R']).trim_start();
    GAINS
        .iter()
        .find(|g| {
            if g.0 == "Spine" {
                name.starts_with("Spine")
            } else {
                name == g.0
            }
        })
        .map_or((100.0, 10.0), |g| (g.1, g.2))
}

/// One hit's softening: each joint's amount and the time since the hit.
#[derive(Clone, Debug)]
struct Soft {
    amount: Vec<f32>,
    root: f32,
    t: f32,
}

/// The muscles of one actor.
#[derive(Clone, Debug)]
pub struct Muscles {
    /// Each joint's base spring and damper (the strength applied).
    pub base: Vec<(f32, f32)>,
    softs: Vec<Soft>,
}

impl Muscles {
    /// The muscles of `model` at `strength` (1 for an average actor; the design's 0.8 to 1.2 from
    /// Strength and Endurance).
    pub fn new(model: &Model, strength: f32) -> Muscles {
        let k = strength.clamp(0.5, 1.5);
        let inertia = joint_inertia(model);
        Muscles {
            base: model
                .joints
                .iter()
                .zip(&inertia)
                .map(|(j, &i)| {
                    let (kp, kd) = base_gains(&model.bodies[j.child].node);
                    let kp = kp * k;
                    // Critically damped for what the joint moves (the table's tenth of the
                    // spring swung the upper body for 1.4 s after a light hit).
                    (kp, (kd * k.sqrt()).max(DAMPING * 2.0 * (kp * i).sqrt()))
                })
                .collect(),
            softs: Vec::new(),
        }
    }

    /// A hit of `impulse` (N·s) on body `struck`.
    pub fn hit(&mut self, model: &Model, struck: usize, impulse: f32) {
        let a = (impulse / SOFT_FULL).clamp(0.0, 1.0) * SOFT_MOST;
        let dist = body_distances(model, struck);
        let amount = model
            .joints
            .iter()
            .map(|j| a * SPREAD.powi(dist[j.child] as i32))
            .collect();
        let root = a * SPREAD.powi(dist[model.root] as i32);
        self.softs.push(Soft {
            amount,
            root,
            t: 0.0,
        });
    }

    /// `dt` on.
    pub fn advance(&mut self, dt: f32) {
        for s in &mut self.softs {
            s.t += dt.max(0.0);
        }
        self.softs.retain(|s| s.t < SOFT_HOLD + RECOVER);
    }

    fn envelope(t: f32) -> f32 {
        if t < SOFT_HOLD {
            1.0
        } else {
            1.0 - ease((t - SOFT_HOLD) / RECOVER)
        }
    }

    /// Each joint's softening now (0 to [`SOFT_MOST`]).
    pub fn softening(&self) -> Vec<f32> {
        let mut out = vec![0.0f32; self.base.len()];
        for s in &self.softs {
            let e = Self::envelope(s.t);
            for (o, a) in out.iter_mut().zip(&s.amount) {
                *o = o.max(a * e);
            }
        }
        out
    }

    /// The root's support's softening now.
    pub fn root_softening(&self) -> f32 {
        self.softs
            .iter()
            .map(|s| s.root * Self::envelope(s.t))
            .fold(0.0, f32::max)
    }

    /// Each joint's spring and damper now.
    pub fn gains(&self) -> Vec<(f32, f32)> {
        self.softening()
            .iter()
            .zip(&self.base)
            .map(|(s, (kp, kd))| (kp * (1.0 - s), kd * (1.0 - 0.5 * s)))
            .collect()
    }

    /// Whether a hit's softening is still on.
    pub fn soft(&self) -> bool {
        !self.softs.is_empty()
    }
}

/// How many joints each body lies from `from` along the body's tree.
pub fn body_distances(model: &Model, from: usize) -> Vec<u32> {
    let n = model.bodies.len();
    let mut dist = vec![u32::MAX; n];
    let mut queue = vec![from];
    if from < n {
        dist[from] = 0;
    }
    let mut i = 0;
    while i < queue.len() {
        let b = queue[i];
        i += 1;
        for j in &model.joints {
            let other = if j.child == b {
                j.parent
            } else if j.parent == b {
                j.child
            } else {
                continue;
            };
            if dist[other] == u32::MAX {
                dist[other] = dist[b] + 1;
                queue.push(other);
            }
        }
    }
    dist.iter().map(|&d| d.min(16)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table reaches every joint; a chest hit of 2 m/s softens the chest's joint by 0.8, the
    /// spine below it by 0.48, the far foot little; it eases back fully by 0.6 s.
    #[test]
    fn a_hit_softens_its_chain_and_eases_back() {
        let m = Model::fnv();
        let mut mu = Muscles::new(&m, 1.0);
        for (j, (kp, _)) in m.joints.iter().zip(&mu.base) {
            assert!(*kp >= 30.0, "{}", m.bodies[j.child].node);
        }
        let chest = m.body("Bip01 Spine2").unwrap();
        mu.hit(&m, chest, 162.0);
        let s = mu.softening();
        let at = |child: &str| {
            let b = m.body(child).unwrap();
            s[m.joints.iter().position(|j| j.child == b).unwrap()]
        };
        assert!((at("Bip01 Spine2") - 0.8).abs() < 1e-4);
        assert!((at("Bip01 Spine1") - 0.48).abs() < 1e-4);
        assert!(at("Bip01 L Foot") < 0.1);
        mu.advance(0.3);
        assert!(at_now(&mu, &m, "Bip01 Spine2") < 0.8 && at_now(&mu, &m, "Bip01 Spine2") > 0.0);
        mu.advance(0.31);
        assert!(!mu.soft());
        assert_eq!(mu.softening().iter().cloned().fold(0.0, f32::max), 0.0);
    }

    fn at_now(mu: &Muscles, m: &Model, child: &str) -> f32 {
        let b = m.body(child).unwrap();
        mu.softening()[m.joints.iter().position(|j| j.child == b).unwrap()]
    }
}
