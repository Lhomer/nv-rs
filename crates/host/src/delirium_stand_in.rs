//! Tests only: a stand-in for Delirium's body (`crate::delirium_np::DlBody`), so the integration
//! (its frames, the put-on, the hand-overs and retakes, the joint limits held, the ground fed, the
//! rise's sequencing) runs in CI, where Delirium cannot (it needs the player's GTA IV). It is a
//! mechanical double, nothing of Delirium's: standing, it takes a light hit as a small shove and a
//! heavy one (or its death) as a fall, tipping over its feet onto the ground in under a second;
//! lying, it settles flat on the ground the world mesh gives it; its muscles on (`rise`/`follow`), its
//! parts go to the pose given, quickly; its parts set or kicked as asked. Metres from the origin,
//! Z up, 21 parts as Delirium's.

use crate::delirium_np::DlBody;
use delirium_bridge::agent::PartState;
use delirium_bridge::delirium::prelude::{Quat, Transform, Vec3};
use delirium_bridge::rig::Rig;
use std::cell::Cell;
use std::sync::OnceLock;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
}

/// Bodies go to the stand-in on this thread (as if Delirium ran).
pub fn set_on(on: bool) {
    ON.with(|o| o.set(on));
}

pub fn on() -> bool {
    ON.with(|o| o.get())
}

/// The stand-in's rig: each of Delirium's parts on its FNV bone as the bind pose has it.
pub fn rig() -> &'static Rig {
    static R: OnceLock<Rig> = OnceLock::new();
    R.get_or_init(|| {
        let s = crate::delirium_np::skel();
        let names: Vec<&str> = s.names.iter().map(String::as_str).collect();
        let bones = delirium_bridge::rig::part_bones(&names);
        let parts: Vec<Transform> = (0..21)
            .map(|p| s.rest[bones[p].or(bones[7]).unwrap()])
            .collect();
        Rig::new(&names, &s.parents, &s.rest, &parts).unwrap()
    })
}

/// A stand-in body at `states`.
pub fn body(states: &[PartState]) -> Box<dyn DlBody> {
    Box::new(StandIn {
        parts: states.iter().map(|s| s.frame).collect(),
        vel: states.iter().map(|s| s.velocity).collect(),
        state: State::Standing,
        dead: false,
        follow: Vec::new(),
        rise: false,
        still: 0.0,
        triangles: Vec::new(),
        limp_for: 0.0,
    })
}

/// A hit's force over which it falls (Delirium's shot force for a hit past the knockdown
/// threshold: `crate::delirium_np::shot_force`).
const FALL_FORCE: f32 = 600.0;
/// Its fall's length (s), its muscles' quickness (each second they close this share of the way,
/// as an exponent), a shove's speed per unit of force (m/s), its parts' radius over the ground (m).
const FALL_FOR: f32 = 0.8;
const MUSCLE: f32 = 9.0;
const SHOVE: f32 = 0.0015;
const RADIUS: f32 = 0.06;
/// How fast a limp body comes down onto the ground under it (m/s).
const SETTLE: f32 = 1.5;

enum State {
    Standing,
    /// Tipping over its feet: seconds in, the axis, the pivot, the pose it started from.
    Falling(f32, Vec3, Vec3, Vec<Transform>),
    Lying,
}

struct StandIn {
    parts: Vec<Transform>,
    vel: Vec<Vec3>,
    state: State,
    dead: bool,
    follow: Vec<Transform>,
    rise: bool,
    still: f32,
    /// The ground's triangles facing up (the world mesh).
    triangles: Vec<[Vec3; 3]>,
    /// How long it has lain limp (s): its arms swing meanwhile.
    limp_for: f32,
}

/// Lying limp, each arm (its clavicle, upper arm, forearm and hand: parts 13 to 16, 17 to 20)
/// swings about its clavicle as Delirium's arms do on the ground, this far (rad) at this rate
/// (rad/s): past FNV's own joint limits (Patrick's PC run on 5f52c0e: an upper arm 85 degrees out
/// of them), the shoulder moved off where FNV's skeleton has it.
const ARM_SWING: f32 = 1.4;
const ARM_RATE: f32 = 2.5;

impl StandIn {
    /// The ground's height under `p` (the highest triangle facing up there).
    fn ground(&self, p: Vec3) -> Option<f32> {
        let mut best: Option<f32> = None;
        for [a, b, c] in &self.triangles {
            let (v0, v1, v2) = (*b - *a, *c - *a, p - *a);
            let d = v0.x * v1.y - v1.x * v0.y;
            if d.abs() < 1e-9 {
                continue;
            }
            let u = (v2.x * v1.y - v1.x * v2.y) / d;
            let v = (v0.x * v2.y - v2.x * v0.y) / d;
            if u >= -1e-4 && v >= -1e-4 && u + v <= 1.0 + 1e-4 {
                let z = a.z + u * v0.z + v * v1.z;
                best = Some(best.map_or(z, |h: f32| h.max(z)));
            }
        }
        best
    }

    /// The whole pose lifted so no part is under the ground; lying limp (no muscles on it), let
    /// down onto it as well (at most [`SETTLE`] m/s), so it lies on the ground, never propped
    /// over it.
    fn onto_ground(&mut self, dt: f32) {
        let lift = self
            .parts
            .iter()
            .filter_map(|p| {
                self.ground(p.translation)
                    .map(|g| g + RADIUS - p.translation.z)
            })
            .reduce(f32::max)
            .unwrap_or(0.0);
        let limp = matches!(self.state, State::Lying) && (self.dead || self.follow.is_empty());
        if lift < 0.0 && limp {
            let down = lift.max(-SETTLE * dt);
            for p in &mut self.parts {
                p.translation.z += down;
            }
        }
        if lift > 0.0 {
            for p in &mut self.parts {
                p.translation.z += lift;
            }
            for v in &mut self.vel {
                v.z = v.z.max(0.0);
            }
        }
    }

    /// It begins to fall along `dir` (flat), over its feet.
    fn fall(&mut self, dir: Vec3) {
        if !matches!(self.state, State::Standing) {
            return;
        }
        let flat = Vec3::new(dir.x, dir.y, 0.0).normalize_or(Vec3::X);
        let feet = (self.parts[3].translation + self.parts[6].translation) * 0.5;
        let axis = Vec3::Z.cross(flat).normalize_or(Vec3::Y);
        self.state = State::Falling(0.0, axis, feet, self.parts.clone());
        self.follow.clear();
        self.rise = false;
    }
}

impl DlBody for StandIn {
    fn set_mesh(&mut self, mesh: &delirium_bridge::world::Mesh) -> Result<(), String> {
        let v = |i: u32| Vec3::from_array(mesh.positions[i as usize]);
        self.triangles = mesh
            .triangles
            .iter()
            .map(|t| [v(t[0]), v(t[1]), v(t[2])])
            .filter(|[a, b, c]| (*b - *a).cross(*c - *a).normalize_or_zero().z > 0.5)
            .collect();
        Ok(())
    }
    fn shot(
        &mut self,
        part: usize,
        _at: Vec3,
        dir: Vec3,
        health: f32,
        force: f32,
    ) -> Result<(), String> {
        let _ = health;
        if matches!(self.state, State::Standing) && !self.dead && force > FALL_FORCE {
            self.fall(dir);
        } else {
            let k = part.min(self.vel.len().saturating_sub(1));
            let push = dir * (force * SHOVE);
            if matches!(self.state, State::Standing) {
                for v in &mut self.vel {
                    *v += push * 0.5;
                }
            } else {
                self.vel[k] += push;
            }
        }
        Ok(())
    }
    fn blast(&mut self, at: Vec3, _radius: f32, _speed: f32) -> Result<(), String> {
        let from = self.parts[0].translation - at;
        self.fall(from);
        Ok(())
    }
    fn kill(&mut self) -> Result<(), String> {
        self.dead = true;
        self.follow.clear();
        self.rise = false;
        let back = self.parts[0].rotation * Vec3::Y;
        self.fall(-back);
        Ok(())
    }
    fn step(&mut self, dt: f32) -> Result<(), String> {
        let mut done_falling = false;
        match &mut self.state {
            State::Falling(t, axis, pivot, start) => {
                *t += dt;
                let a = (*t / FALL_FOR).min(1.0) * std::f32::consts::FRAC_PI_2;
                let q = Quat::from_axis_angle(*axis, a);
                for (p, s) in self.parts.iter_mut().zip(start.iter()) {
                    p.translation = *pivot + q * (s.translation - *pivot);
                    p.rotation = (q * s.rotation).normalize();
                }
                done_falling = *t >= FALL_FOR;
            }
            State::Standing | State::Lying => {
                let decay = (-6.0 * dt).exp();
                for (p, v) in self.parts.iter_mut().zip(self.vel.iter_mut()) {
                    p.translation += *v * dt;
                    *v *= decay;
                }
                if !self.dead && !self.follow.is_empty() && self.follow.len() == self.parts.len() {
                    let k = 1.0 - (-(if self.rise { MUSCLE } else { MUSCLE * 0.4 }) * dt).exp();
                    for (p, f) in self.parts.iter_mut().zip(&self.follow) {
                        p.translation = p.translation.lerp(f.translation, k);
                        p.rotation = p.rotation.slerp(f.rotation, k).normalize();
                    }
                    self.still = 0.0;
                } else if matches!(self.state, State::Lying) {
                    self.still += dt;
                }
            }
        }
        if done_falling {
            self.state = State::Lying;
            self.still = 0.0;
            self.vel.fill(Vec3::ZERO);
        }
        // Its arms swing while it lies limp, each about its clavicle, the two out of step.
        let limp = matches!(self.state, State::Lying) && (self.dead || self.follow.is_empty());
        if limp && self.parts.len() == 21 {
            let before = ARM_SWING * (ARM_RATE * self.limp_for).sin();
            self.limp_for += dt;
            let now = ARM_SWING * (ARM_RATE * self.limp_for).sin();
            let across = self.parts[10].rotation * Vec3::X;
            for (arm, sign) in [(13usize, 1.0f32), (17, -1.0)] {
                let q = Quat::from_axis_angle(across, sign * (now - before));
                let pivot = self.parts[arm].translation;
                for k in arm..arm + 4 {
                    let p = &mut self.parts[k];
                    p.translation = pivot + q * (p.translation - pivot);
                    p.rotation = (q * p.rotation).normalize();
                }
            }
        } else {
            self.limp_for = 0.0;
        }
        // Up again on its feet (its muscles brought it there): standing.
        if matches!(self.state, State::Lying) && !self.dead && self.rise {
            let up = self.parts[12].translation - self.parts[0].translation;
            if up.normalize_or_zero().z > 0.95 {
                self.state = State::Standing;
            }
        }
        self.onto_ground(dt);
        Ok(())
    }
    fn parts(&self) -> Result<Vec<Transform>, String> {
        Ok(self.parts.clone())
    }
    fn balancing(&self) -> bool {
        matches!(self.state, State::Standing) && !self.dead && self.follow.is_empty()
    }
    fn should_get_up(&self, after: f32) -> bool {
        matches!(self.state, State::Lying) && !self.dead && self.still >= after
    }
    fn relax(&mut self) -> Result<(), String> {
        self.follow.clear();
        self.rise = false;
        Ok(())
    }
    fn rise(&mut self, on: bool, _help: f32) -> Result<(), String> {
        self.rise = on;
        Ok(())
    }
    fn follow(&mut self, parts: &[Transform]) -> Result<(), String> {
        self.follow = parts.to_vec();
        Ok(())
    }
    fn motions(&self) -> Result<Vec<(Vec3, Vec3)>, String> {
        Ok(self.vel.iter().map(|v| (*v, Vec3::ZERO)).collect())
    }
    fn set_part(&mut self, part: usize, state: &PartState) -> Result<(), String> {
        if let (Some(p), Some(v)) = (self.parts.get_mut(part), self.vel.get_mut(part)) {
            *p = state.frame;
            *v = state.velocity;
        }
        Ok(())
    }
    fn kick(&mut self, dv: Vec3) -> Result<(), String> {
        for v in &mut self.vel {
            *v += dv;
        }
        Ok(())
    }
}
