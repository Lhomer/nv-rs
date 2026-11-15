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
use ragdoll_core::balance::{
    between, classify, plan, two_bone, Ground as Step, Limbs, Outcome, Stepping,
};
use ragdoll_core::flinch::*;
use ragdoll_core::knock::{
    add_hit, body_mass, launch_direction, launch_speed, reaction, topple_speed, transferred, Body,
    Push, Reaction, HOLD_SECONDS, LYING_HEIGHT, STILL_FRAMES, STILL_SPEED,
};
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
/// The get-up's blend back to the rest pose (s).
pub const GET_UP: f32 = 1.6;
/// How far from the body the scripted and clicked shots come (m), and how wide a shotgun's
/// pattern grows a metre (units of spread radius per metre of range).
pub const PELLET_SPREAD: f32 = 2.4;

/// Hands reaching to a wall along `dir` (stage 3), `t` seconds in.
#[derive(Clone, Copy, Debug)]
pub struct Brace {
    pub dir: V3,
    pub t: f32,
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
}

/// The rig (the host's `active_ragdoll::Rig`): each bone's spring, the pelvis's drop, the steps,
/// the brace and the fall.
#[derive(Clone, Debug)]
pub struct Rig {
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
}

impl Rig {
    fn new(limbs: Limbs) -> Rig {
        Rig {
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
    /// The body holds a weapon (aims it) and is attacking (an attack a big hit interrupts).
    pub armed: bool,
    pub attacking: bool,
    pub t: f64,
    pub pos: V3,
    pub rig: Rig,
    pub state: State,
    pub stack: Option<Stack>,
    pub last_knock: Option<f64>,
    pub last_up: Option<f64>,
    pub posed: [Xf; COUNT],
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
            armed: false,
            attacking: false,
            t: 0.0,
            pos: [0.0; 3],
            rig: Rig::new(Limbs::WHOLE),
            state: State::Standing,
            stack: None,
            last_knock: None,
            last_up: None,
            posed: [IDENT_XF; COUNT],
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
        self.rig = Rig::new(self.rig.limbs);
        self.state = State::Standing;
        self.stack = None;
        self.last_knock = None;
        self.last_up = None;
        self.stats = Stats {
            start,
            ..Stats::default()
        };
        self.t = 0.0;
        self.posed = self.anim();
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
        let mut out = [IDENT_XF; COUNT];
        for b in 0..COUNT {
            let (prot, ppos) = match mq::PARENT[b] {
                Some(p) => out[p],
                None => (IDENTITY, self.pos),
            };
            out[b] = (
                mul(&prot, &self.anim_local(b)),
                add(ppos, apply_m(&prot, self.rest_local(b))),
            );
        }
        out
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
        let decision = reaction(speed, topple);
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
                self.flinch(p, mass, point, direction, false);
                self.interrupt_attack("knocked down");
                if !self.fall(point, stacked.direction, speed) {
                    self.knock_down(stacked.direction, speed, "knockdown");
                }
            }
            Reaction::Stagger => {
                self.flinch(p, mass, point, direction, false);
                self.interrupt_attack("staggered");
                self.push(p, mass, direction);
            }
            Reaction::None => {
                self.flinch(p, mass, point, direction, true);
                self.push(p, mass, direction);
            }
        }
    }

    fn nearest_bone(&self, point: V3) -> usize {
        (0..COUNT)
            .min_by(|&a, &b| {
                length(sub(self.posed[a].1, point))
                    .partial_cmp(&length(sub(self.posed[b].1, point)))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(0)
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
        self.rig.since = self.t;
        self.rig.active = true;
        self.rig.aim_hold = if self.armed && keep_aim {
            t.aim_hold
        } else {
            0.0
        };
        let spread = spread_of(v);
        let positions: Vec<V3> = self.posed.iter().map(|x| x.1).collect();
        let struck = self.nearest_bone(point);
        let (struck_role, _) = mq::role_side(struck);
        let mut touched = 0;
        let mut spine_kick: Option<V3> = None;
        for (i, pos) in positions.iter().enumerate() {
            let (role, _) = mq::role_side(i);
            let distance = length(sub(*pos, point)) / UNITS_PER_METRE;
            let w = weight(role, distance, spread);
            if w < 0.02 {
                continue;
            }
            let lever = sub(point, *pos);
            let lever = if length(lever) < 5.0 {
                [0.0, 0.0, 1.0]
            } else {
                lever
            };
            let Some(axis) = unit(cross(lever, direction)) else {
                continue;
            };
            let mut kick = scale(axis, t.kick * v * w);
            if role == Role::Head {
                if let Some(s) = spine_kick {
                    kick = add(kick, scale(s, -t.head_lag));
                }
            }
            if role == Role::Spine {
                spine_kick = Some(kick);
            }
            let s = &mut self.rig.springs[i];
            s.velocity = add(s.velocity, kick);
            s.dip = (s.dip + t.muscle_dip * w.min(1.0)).min(0.9);
            touched += 1;
        }
        if matches!(struck_role, Role::Thigh | Role::Calf) {
            let across = unit(cross([0.0, 0.0, 1.0], direction)).unwrap_or([1.0, 0.0, 0.0]);
            for i in 0..COUNT {
                let (role, _) = mq::role_side(i);
                if !matches!(role, Role::Thigh | Role::Calf) {
                    continue;
                }
                if !(length(sub(positions[i], positions[struck])) < 60.0 || i == struck) {
                    continue;
                }
                let sign = if role == Role::Calf { -1.0 } else { 1.0 };
                let s = &mut self.rig.springs[i];
                s.velocity = add(s.velocity, scale(across, sign * t.knee_buckle * v));
            }
            self.rig.drop_speed -= t.pelvis_drop * v;
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
        let scale_ = self.scale;
        let one_leg = !self.rig.limbs.legs();
        let anim = self.anim();
        let pelvis = add(anim[mq::PELVIS].1, self.rig.shift);
        let (mut feet, carried) = match &self.rig.balance {
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
            });
            self.rig.balance = None;
            self.note(format!("on one leg it cannot step: falls ({v:.2} m/s)"));
            return;
        }
        let mut outcome =
            if planned.need > self.tune.step_length * scale_ * self.tune.max_steps() as f32 {
                Outcome::Overrun
            } else {
                Outcome::Caught
            };
        let start = feet;
        let mut steps = Vec::new();
        let mut said = Vec::new();
        let ankle: [f32; 2] = [0, 1].map(|f| {
            world::ground_point(&self.world, start[f])
                .map(|g| start[f][2] - g[2])
                .filter(|h| (0.0..=25.0).contains(h))
                .unwrap_or(8.0 * scale_)
        });
        for (foot, to) in planned.steps {
            let from = feet[foot];
            let ground = classify(to, from[2], world::ground_point(&self.world, to));
            match ground {
                Step::Floor(z) => {
                    let to = [to[0], to[1], z + ankle[foot]];
                    steps.push((foot, to));
                    feet[foot] = to;
                }
                Step::Ledge => {
                    steps.push((foot, [to[0], to[1], from[2]]));
                    outcome = Outcome::Ledge;
                    said.push("a ledge");
                    break;
                }
                Step::Blocked => {
                    let mut placed = false;
                    for k in [0.66f32, 0.33] {
                        let short = add(from, scale(sub(to, from), k));
                        if let Step::Floor(z) =
                            classify(short, from[2], world::ground_point(&self.world, short))
                        {
                            steps.push((foot, [short[0], short[1], z + ankle[foot]]));
                            placed = true;
                            break;
                        }
                    }
                    outcome = Outcome::Wall;
                    said.push(if placed {
                        "a wall: the step shortened"
                    } else {
                        "a wall: no room to step"
                    });
                    break;
                }
            }
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
            self.rig.brace = Some(Brace { dir, t: 0.0 });
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

    fn start_brace(&mut self, dir: V3, v: f32) {
        let limit = self.tune.brace_holds * self.tune.balance.max(1.0) / 100.0;
        let arms = self.rig.limbs.arm(0) || self.rig.limbs.arm(1);
        if arms {
            self.rig.brace = Some(Brace { dir, t: 0.0 });
            self.stats.braced = true;
        }
        if v > limit || !arms {
            let point = self.anim()[mq::PELVIS].1;
            self.rig.fall = Some(Fall {
                dir,
                speed: v * 0.5,
                point,
                t: 0.0,
                lead: self.tune.brace_reach + 0.1,
                windmill: false,
                why: "crumpled against a wall",
            });
        }
        self.note(format!(
            "braces against a wall with {}{}",
            if arms {
                "its hands"
            } else {
                "no arm to brace with"
            },
            if v > limit || !arms {
                format!(": the push ({v:.2} m/s) is too much, it crumples into its ragdoll")
            } else {
                String::new()
            }
        ));
    }

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
        let shoulder = anim[u].1;
        let elbow0 = anim[f].1;
        let hand0 = anim[h].1;
        let (l1, l2) = (length(sub(elbow0, shoulder)), length(sub(hand0, elbow0)));
        if l1 < 1.0 || l2 < 1.0 {
            return None;
        }
        let goal = add(hand0, scale(sub(target, hand0), w.clamp(0.0, 1.0)));
        let pole = add(elbow0, [0.0, 0.0, -20.0]);
        let (elbow, reached) = two_bone(shoulder, goal, l1, l2, pole)?;
        let r1 = rotation(between(sub(elbow0, shoulder), sub(elbow, shoulder)));
        let r2 = rotation(between(
            apply_m(&r1, sub(hand0, elbow0)),
            sub(reached, elbow),
        ));
        Some((u, r1, f, mul(&mul(&transpose(&r1), &r2), &r1)))
    }

    fn brace_frame(&mut self, anim: &[Xf; COUNT], out: &mut [Option<M3>], dt: f32) {
        let Some(mut b) = self.rig.brace.take() else {
            return;
        };
        b.t += dt;
        let Some(w) = brace_weight(b.t) else {
            self.note("lets go of the wall".into());
            return;
        };
        for side in 0..2u8 {
            if !self.rig.limbs.arm(side as usize) {
                continue;
            }
            let Some(u) = mq::bone_of(Role::UpperArm, side) else {
                continue;
            };
            let shoulder = anim[u].1;
            let target = add(
                shoulder,
                add(scale(b.dir, 0.55 * UNITS_PER_METRE), [0.0, 0.0, -8.0]),
            );
            if let Some((ui, ur, fi, fr)) = self.reach_arm(anim, side, target, w) {
                out[ui] = Some(ur);
                out[fi] = Some(fr);
            }
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
        let k = (f.t / f.lead.max(1e-3)).clamp(0.0, 1.0);
        let across = unit(cross([0.0, 0.0, 1.0], f.dir)).unwrap_or([1.0, 0.0, 0.0]);
        let lean = if f.windmill { -0.3 } else { t.fall_lean * k };
        if let Some(i) = mq::bone_of(Role::Spine, 2) {
            out[i] = Some(rotation(scale(across, lean)));
        }
        for side in 0..2u8 {
            if let Some(b) = mq::bone_of(Role::Thigh, side) {
                out[b] = Some(rotation(scale(across, t.fall_knees * k)));
            }
            if let Some(c) = mq::bone_of(Role::Calf, side) {
                out[c] = Some(rotation(scale(across, -t.fall_knees * k)));
            }
            if f.windmill && self.rig.limbs.arm(side as usize) {
                if let Some(u) = mq::bone_of(Role::UpperArm, side) {
                    let phase = if side == 0 { 0.0 } else { std::f32::consts::PI };
                    let a = t.windmill_swing
                        * (std::f32::consts::TAU * t.windmill_hz * f.t + phase).sin();
                    out[u] = Some(rotation(scale(across, a)));
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
        self.knock_down(f.dir, f.speed, f.why);
        true
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
        self.rig.fall = Some(Fall {
            dir,
            speed,
            point,
            t: 0.0,
            lead: self.tune.fall_lead,
            windmill: false,
            why: "knockdown",
        });
        self.rig.active = true;
        self.rig.since = self.t;
        true
    }

    // --- Stage 2's frame (the host's `balance_frame`). -------------------------------------------

    fn balance_frame(&mut self, dt: f32) -> Vec<Option<M3>> {
        let mut out: Vec<Option<M3>> = vec![None; COUNT];
        let Some(mut s) = self.rig.balance.take() else {
            self.rig.shift = [0.0; 3];
            return out;
        };
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
            let (hip, knee_now, ankle) = (add(hip0, shift), add(knee0, shift), add(ankle0, shift));
            let target = add(ankle, scale(sub(feet[side as usize], ankle), s.weight));
            let bend = sub(knee_now, scale(add(hip, ankle), 0.5));
            let pole = if length(bend) > 1.0 {
                add(knee_now, scale(bend, 2.0))
            } else {
                add(knee_now, scale(s.dir, 20.0))
            };
            let Some((knee, reached)) = two_bone(hip, target, l1, l2, pole) else {
                continue;
            };
            let r1 = rotation(between(sub(knee0, hip0), sub(knee, hip)));
            let r2 = rotation(between(
                apply_m(&r1, sub(ankle0, knee0)),
                sub(reached, knee),
            ));
            let m = mul(&mul(&transpose(&r1), &r2), &r1);
            let level = mul(&transpose(&m), &transpose(&r1));
            out[t] = Some(r1);
            out[c] = Some(m);
            out[f] = Some(level);
        }
        let spine = anim[mq::SPINE].1;
        for side in 0..2u8 {
            if !self.rig.limbs.arm(side as usize) || s.arms <= 0.0 {
                continue;
            }
            let (Some(u), Some(f)) = (
                mq::bone_of(Role::UpperArm, side),
                mq::bone_of(Role::Forearm, side),
            ) else {
                continue;
            };
            let (shoulder, elbow) = (anim[u].1, anim[f].1);
            let outward = [shoulder[0] - spine[0], shoulder[1] - spine[1], 0.0];
            if let Some(axis) = unit(cross(sub(elbow, shoulder), outward)) {
                out[u] = Some(rotation(scale(axis, self.tune.arms_out * s.arms)));
            }
        }
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
            });
            return out;
        }
        if s.done(self.t) {
            self.note(format!(
                "steps done ({:?}): back to its animation",
                s.outcome
            ));
            self.rig.shift = [0.0; 3];
        } else {
            self.rig.balance = Some(s);
        }
        out
    }

    // --- The frame (the host's `active_ragdoll::apply`). -----------------------------------------

    /// One step of the simulation.
    pub fn step(&mut self) {
        tuning::set(self.tune);
        let dt = STEP;
        self.t += dt as f64;
        match self.state {
            State::Standing => self.pose_frame(dt),
            State::Ragdoll { .. } => self.ragdoll_frame(),
            State::GettingUp { .. } => self.get_up_frame(dt),
        }
        if self.stats.recovered_at.is_none() && self.stats.last_hit.is_some() && self.recovered() {
            self.stats.recovered_at = Some(self.t);
            let since = self.t - self.stats.first_hit.unwrap_or(self.t);
            self.note(format!("recovered: {since:.2} s after the first hit"));
        }
    }

    /// Back on its feet and at rest.
    pub fn recovered(&self) -> bool {
        self.standing() && !self.rig.active
    }

    fn pose_frame(&mut self, dt: f32) {
        let mut overrides: Vec<Option<M3>> = vec![None; COUNT];
        if self.rig.active {
            for b in 0..COUNT {
                let (role, _) = mq::role_side(b);
                self.rig.springs[b].step(role, dt);
            }
            overrides = self.balance_frame(dt);
            let anim = self.anim();
            self.brace_frame(&anim, &mut overrides, dt);
            if self.fall_frame(&mut overrides, dt) {
                return;
            }
            let (f, zeta) = Role::Pelvis.spring();
            let w0 = std::f32::consts::TAU * f;
            self.rig.drop_speed +=
                (-w0 * w0 * self.rig.drop - 2.0 * zeta * w0 * self.rig.drop_speed) * dt;
            self.rig.drop =
                (self.rig.drop + self.rig.drop_speed * dt).clamp(-self.tune.pelvis_drop_max, 0.0);
            // The arms follow the poses they are given (or their own) with a lag.
            for (b, o) in overrides.iter().enumerate() {
                if mq::role_side(b).0.is_arm() {
                    let target = o.map_or([0.0; 3], |o| rotation_vector(&o));
                    self.rig.follow[b].step(target, dt);
                }
            }
        }
        let resting = self.rig.springs.iter().all(|s| s.at_rest())
            && self.rig.follow.iter().all(|f| f.at_rest())
            && self.rig.drop.abs() < 1e-3
            && self.rig.balance.is_none()
            && self.rig.brace.is_none()
            && self.rig.fall.is_none();
        // Each bone's world offset: its pose override (an arm's followed) on its spring's, the
        // aim kept for an armed body (the host's `apply`).
        let mut offsets: Vec<M3> = (0..COUNT)
            .map(|b| {
                let spring = rotation(self.rig.springs[b].angle);
                let o = if mq::role_side(b).0.is_arm() {
                    Some(rotation(self.rig.follow[b].angle))
                } else {
                    overrides[b]
                };
                match o {
                    Some(o) => mul(&o, &spring),
                    None => spring,
                }
            })
            .collect();
        if self.armed && self.rig.aim_hold > 0.0 {
            let roles: Vec<Role> = (0..COUNT).map(|b| mq::role_side(b).0).collect();
            hold_aim(&mut offsets, &mq::PARENT, &roles, self.rig.aim_hold);
        }
        let before = self.posed;
        self.posed = self.pose(&offsets, resting);
        self.measure(&before, dt);
        if self.rig.active && resting && self.t - self.rig.since > 0.2 {
            self.rig.active = false;
            self.rig.springs = [Spring::REST; COUNT];
            self.rig.shift = [0.0; 3];
        }
    }

    /// The pose layer's write and the update: each bone's local rotation `P^T (O S) P L` over the
    /// animated pose (`P` its parent's animated world rotation), the pelvis carried by the drop
    /// and the shift; the world transforms brought up from the locals.
    fn pose(&self, offsets: &[M3], resting: bool) -> [Xf; COUNT] {
        let anim = self.anim();
        let mut out = [IDENT_XF; COUNT];
        for b in 0..COUNT {
            let p_anim = mq::PARENT[b].map_or(IDENTITY, |p| anim[p].0);
            let pose_rot = self.anim_local(b);
            let mut t = self.rest_local(b);
            let rot = if resting {
                pose_rot
            } else {
                let r = offsets[b];
                if b == mq::PELVIS {
                    t = add(
                        t,
                        apply_m(
                            &transpose(&p_anim),
                            add([0.0, 0.0, self.rig.drop * UNITS_PER_METRE], self.rig.shift),
                        ),
                    );
                }
                mul(&mul(&mul(&transpose(&p_anim), &r), &p_anim), &pose_rot)
            };
            let (prot, ppos) = match mq::PARENT[b] {
                Some(p) => out[p],
                None => (IDENTITY, self.pos),
            };
            out[b] = (mul(&prot, &rot), add(ppos, apply_m(&prot, t)));
        }
        out
    }

    /// The run's measures for a standing frame: the arms' turning speed, the weapon hand's drift
    /// off its aim, the flinch's largest turn.
    fn measure(&mut self, before: &[Xf; COUNT], dt: f32) {
        if self.stats.last_hit.is_none() || dt <= 0.0 {
            return;
        }
        let anim = self.anim();
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
        }
        if self.armed {
            let h = mq::R_HAND;
            let off = length(rotation_vector(&mul(
                &transpose(&anim[h].0),
                &self.posed[h].0,
            )));
            self.stats.aim_drift = self.stats.aim_drift.max(off.to_degrees());
        }
    }

    // --- The ragdoll (the host's `knockdown::knock_down`, the launch, the get-up). ---------------

    fn knock_down(&mut self, direction: V3, body_speed: f32, why: &str) {
        if let Some(at) = self.last_knock.filter(|at| self.t - at < KNOCK_COOLDOWN) {
            self.note(format!(
                "duplicate guard: knocked down {:.1} s ago, not knocked again",
                self.t - at
            ));
            self.rig.fall = None;
            return;
        }
        if let Some(at) = self.last_up.filter(|at| self.t - at < UP_COOLDOWN) {
            self.note(format!(
                "duplicate guard: got up {:.1} s ago, not knocked again",
                self.t - at
            ));
            self.rig.fall = None;
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
            let rb = RigidBodyBuilder::dynamic()
                .pose(Pose::from_parts(to_m(pos), quat_of(&rot)))
                .linvel(v)
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
        self.last_knock = Some(self.t);
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
        let lying = high - low <= LYING_HEIGHT * self.scale;
        let held = (self.t - since) as f32 >= HOLD_SECONDS;
        if (*still >= STILL_FRAMES && lying) || held {
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
        let k = k * k * (3.0 - 2.0 * k);
        let anim = self.anim();
        for b in 0..COUNT {
            let qa = quat_of(&from[b].0);
            let qb = quat_of(&anim[b].0);
            let q = qa.slerp(qb, k);
            let p = add(from[b].1, scale(sub(anim[b].1, from[b].1), k));
            self.posed[b] = (m3_of(q), p);
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
                let Some((round, pellets)) = weapons::round(gun, load) else {
                    return;
                };
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
