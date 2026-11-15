//! Active ragdoll, stage 1: the physics flinch (Patrick, 2026-11-15: a GTA IV / Red Dead style
//! active ragdoll, in stages; stage 2 balance and procedural stepping, stage 3 bracing and falling
//! into the ragdoll come later). Part of the weighty combat row; `[nv-rs Gunplay] iHitReactions`
//! (percent, 0 = off; Settings > Gameplay "Hit Reactions"). **Not 1:1 with FalloutNV.exe.**
//!
//! **What it does.** A hit on a living actor below the knockdown's topple speed
//! ([`crate::knockdown`]) kicks its skeleton: the struck bone (the rig's bone nearest the impact
//! point) along the hit's direction, and the kick spreads through the body, fading with distance
//! from the impact ([`spread_of`]: farther for a bigger hit) and with each bone's mass ([`Role`]):
//! a shoulder hit swings the shoulder back, twists the spine, turns the pelvis a little, lags and
//! snaps the head and lets the arms swing; a low torso hit turns the hips more; a leg hit buckles
//! the knee and drops the pelvis. Each bone then springs back to its animated pose with a little
//! overshoot (a spring-damper per bone, under-damped, [`Role::spring`]), its muscle (stiffness)
//! lowered on and near the hit for a moment and raised back ([`MUSCLE_DIP`]). Hits layer: each adds
//! to the bones' motion.
//!
//! **How much.** Per hit, never the stack: the momentum the hit transfers and the body's mass as
//! the knockdowns work them out ([`crate::knockdown::measure_hit`], the ragdoll's own Havok mass
//! when measured; effective weapon stats with their mods), the body's speed `v = p / mass` times
//! `iHitReactions` percent; a bone's kick is [`KICK`] radians a second per m/s, times its weight.
//!
//! **How it shows (the pose layer, [`apply`]).** A living actor's bones follow its animation (its
//! ragdoll's bodies are keyframed to them), so stage 1 does not power its Havok joints: switching a
//! standing actor's ragdoll into a powered blend is what made the knockdowns' get-ups unstable.
//! Instead, each frame at the draw's set-up (`Main::DrawWorld_MTSetup1stPerson`, `00874c10`, after
//! the animation posed the actors), each kicked bone's local rotation is turned by its spring's
//! offset (a rotation in world space, carried to the bone's frame through its parent's world
//! rotation; children inherit it), and the actor's 3D is updated by the game's own update
//! (`NiAVObject::Update`, `00a59c60`, with an update data of no time, as `KnockExplosion` does).
//! The animation writes the bones afresh the next frame; a bone the animation did not write again
//! (a paused game, a menu) is put back to its pose before the offset is applied again, and when
//! the springs come to rest the pose is put back. The actor's reference and its character
//! controller are never moved (Patrick's rule).
//!
//! **Never:** the player, a dead actor, one knocked down or getting up (`crate::knockdown`), a
//! companion unless `bKnockdownCompanions`, an actor below the high process (no animated 3D).
//!
//! **For stage 2 and 3:** [`Rig`] (the skeleton's named bones), [`Spring`] (a bone's offset state)
//! and [`apply`] (the pose layer) are what balance and bracing drive too; [`hit`] is the stimulus
//! entry, and stage 3's fall into the ragdoll goes through `crate::knockdown::knock_down`.

use engine::prelude::*;
use std::sync::Mutex;
use std::time::Instant;

/// A bone's role: its spring, its share of a kick (light parts move more), how far it may turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Pelvis,
    Spine,
    Neck,
    Head,
    Clavicle,
    UpperArm,
    Forearm,
    Thigh,
    Calf,
    Foot,
}

impl Role {
    /// The spring's frequency (Hz) and damping ratio (under 1: an overshoot).
    pub fn spring(self) -> (f32, f32) {
        match self {
            Role::Pelvis => (3.2, 0.5),
            Role::Spine => (3.0, 0.45),
            Role::Neck => (2.6, 0.4),
            Role::Head => (2.2, 0.35),
            Role::Clavicle => (2.4, 0.4),
            Role::UpperArm => (1.6, 0.3),
            Role::Forearm => (1.8, 0.3),
            Role::Thigh => (3.0, 0.5),
            Role::Calf => (3.2, 0.5),
            Role::Foot => (3.4, 0.6),
        }
    }

    /// How much of a kick the bone takes (its lightness).
    pub fn gain(self) -> f32 {
        match self {
            Role::Pelvis => 0.35,
            Role::Spine => 0.7,
            Role::Neck => 0.8,
            Role::Head => 0.9,
            Role::Clavicle => 1.0,
            Role::UpperArm => 1.2,
            Role::Forearm => 1.1,
            Role::Thigh => 0.6,
            Role::Calf => 0.7,
            Role::Foot => 0.3,
        }
    }

    /// The most the bone turns (radians).
    pub fn limit(self) -> f32 {
        match self {
            Role::Pelvis => 0.2,
            Role::Spine => 0.35,
            Role::Neck => 0.4,
            Role::Head => 0.5,
            Role::Clavicle => 0.45,
            Role::UpperArm => 0.9,
            Role::Forearm => 0.7,
            Role::Thigh => 0.45,
            Role::Calf => 0.6,
            Role::Foot => 0.4,
        }
    }
}

/// The rig's bones by name (the Gamebryo biped's), and their roles.
pub const BONES: [(&str, Role); 18] = [
    ("Bip01 Pelvis", Role::Pelvis),
    ("Bip01 Spine", Role::Spine),
    ("Bip01 Spine1", Role::Spine),
    ("Bip01 Spine2", Role::Spine),
    ("Bip01 Neck1", Role::Neck),
    ("Bip01 Head", Role::Head),
    ("Bip01 L Clavicle", Role::Clavicle),
    ("Bip01 R Clavicle", Role::Clavicle),
    ("Bip01 L UpperArm", Role::UpperArm),
    ("Bip01 R UpperArm", Role::UpperArm),
    ("Bip01 L Forearm", Role::Forearm),
    ("Bip01 R Forearm", Role::Forearm),
    ("Bip01 L Thigh", Role::Thigh),
    ("Bip01 R Thigh", Role::Thigh),
    ("Bip01 L Calf", Role::Calf),
    ("Bip01 R Calf", Role::Calf),
    ("Bip01 L Foot", Role::Foot),
    ("Bip01 R Foot", Role::Foot),
];

/// The tuning (module documentation): kick (rad/s per m/s of the body's speed), the spread's
/// reach (m) at no speed and per m/s, at most; the muscle's dip near the hit and how fast it comes
/// back (s); the head's lag (a share of the spine's kick against it); the knee's buckle and the
/// pelvis's drop (m/s per m/s) for a leg hit; when a spring is at rest; the frame time at most.
pub const KICK: f32 = 16.0;
pub const SPREAD_BASE: f32 = 0.25;
pub const SPREAD_PER_MPS: f32 = 0.4;
pub const SPREAD_MAX: f32 = 1.5;
pub const MUSCLE_DIP: f32 = 0.6;
pub const MUSCLE_BACK: f32 = 0.25;
pub const HEAD_LAG: f32 = 0.6;
pub const KNEE_BUCKLE: f32 = 6.0;
pub const PELVIS_DROP: f32 = 0.6;
pub const PELVIS_DROP_MAX: f32 = 0.12;
pub const REST: f32 = 0.002;
pub const MAX_DT: f32 = 0.05;
pub const UNITS_PER_METRE: f32 = 70.0;

/// How far (m) a kick spreads for a body speed `v` (m/s).
pub fn spread_of(v: f32) -> f32 {
    (SPREAD_BASE + SPREAD_PER_MPS * v.max(0.0)).min(SPREAD_MAX)
}

/// A bone's weight in a kick: its role's gain, fading with its distance (m) from the impact.
pub fn weight(role: Role, distance: f32, spread: f32) -> f32 {
    role.gain() * (-distance.max(0.0) / spread.max(0.05)).exp()
}

// --- Vectors and matrices (row-major 3 x 3, as `NiMatrix3`). -----------------------------------

type V3 = [f32; 3];
type M3 = [f32; 9];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> Option<V3> {
    let l = length(a);
    (l.is_finite() && l > 1e-6).then(|| scale(a, 1.0 / l))
}

fn mul(a: &M3, b: &M3) -> M3 {
    let mut m = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            m[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    m
}
fn transpose(a: &M3) -> M3 {
    [a[0], a[3], a[6], a[1], a[4], a[7], a[2], a[5], a[8]]
}
fn apply_m(a: &M3, v: V3) -> V3 {
    [
        a[0] * v[0] + a[1] * v[1] + a[2] * v[2],
        a[3] * v[0] + a[4] * v[1] + a[5] * v[2],
        a[6] * v[0] + a[7] * v[1] + a[8] * v[2],
    ]
}

/// The rotation by the rotation vector `w` (its direction the axis, its length the angle).
pub fn rotation(w: V3) -> M3 {
    let angle = length(w);
    if !angle.is_finite() || angle < 1e-7 {
        return [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    }
    let [x, y, z] = scale(w, 1.0 / angle);
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    [
        t * x * x + c,
        t * x * y - s * z,
        t * x * z + s * y,
        t * x * y + s * z,
        t * y * y + c,
        t * y * z - s * x,
        t * x * z - s * y,
        t * y * z + s * x,
        t * z * z + c,
    ]
}

// --- The springs. --------------------------------------------------------------------------------

/// A bone's offset from its animated pose: a rotation vector (world space) and its angular
/// velocity, its muscle's dip and when it was hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub angle: V3,
    pub velocity: V3,
    pub dip: f32,
}

impl Spring {
    pub const REST: Spring = Spring {
        angle: [0.0; 3],
        velocity: [0.0; 3],
        dip: 0.0,
    };

    /// One step of `dt` seconds of a bone of `role`: pulled back to its pose (its muscle lowered by
    /// the dip, which fades), damped, held within the role's limit.
    pub fn step(&mut self, role: Role, dt: f32) {
        let (f, zeta) = role.spring();
        let w0 = std::f32::consts::TAU * f;
        let muscle = 1.0 - self.dip.clamp(0.0, 0.9);
        let k = w0 * w0 * muscle;
        let c = 2.0 * zeta * w0 * muscle.sqrt();
        let dt = dt.clamp(0.0, MAX_DT);
        // Semi-implicit Euler, in two halves for the stiff springs.
        for _ in 0..2 {
            let h = dt / 2.0;
            let acc = sub(scale(self.angle, -k), scale(self.velocity, c));
            self.velocity = add(self.velocity, scale(acc, h));
            self.angle = add(self.angle, scale(self.velocity, h));
        }
        let a = length(self.angle);
        if a > role.limit() {
            self.angle = scale(self.angle, role.limit() / a);
            // At the limit the joint stops it: the velocity outward is taken off.
            if let Some(u) = unit(self.angle) {
                let out = dot(self.velocity, u);
                if out > 0.0 {
                    self.velocity = sub(self.velocity, scale(u, out));
                }
            }
        }
        self.dip *= (-dt / MUSCLE_BACK).exp();
    }

    pub fn at_rest(&self) -> bool {
        length(self.angle) < REST && length(self.velocity) < REST * 10.0
    }
}

// --- The rig and the flinching actors. -----------------------------------------------------------

/// An actor's rig: each found bone's node, its role and its spring; the pelvis's drop (m) and its
/// speed (m/s); what the pose layer wrote and the pose it wrote over.
#[derive(Clone, Debug)]
pub struct Rig {
    pub actor: u32,
    pub id: u32,
    pub root: u32,
    pub bones: Vec<Bone>,
    pub drop: f32,
    pub drop_speed: f32,
    pub since: Instant,
    /// Stage 2: the catch steps under way ([`crate::balance`]), the pelvis's offset they carry
    /// (world, game units), and the limbs the body still has (dismemberment changes them).
    pub balance: Option<crate::balance::Stepping>,
    pub shift: V3,
    pub limbs: crate::balance::Limbs,
}

#[derive(Clone, Copy, Debug)]
pub struct Bone {
    pub node: u32,
    pub role: Role,
    /// 0 left, 1 right, 2 the middle.
    pub side: u8,
    pub spring: Spring,
    /// The animated pose (local rotation, local translation) and what was written over it.
    pub pose: Option<(M3, V3)>,
    pub written: Option<(M3, V3)>,
}

static RIGS: Mutex<Vec<Rig>> = Mutex::new(Vec::new());
static LAST: Mutex<Option<Instant>> = Mutex::new(None);

// --- The game's words. ---------------------------------------------------------------------------

const ACTOR_3D: u32 = 0x0043_fcd0;
const CHILD_COUNT: u32 = 0x0043_b480;
const CHILD_AT: u32 = 0x0043_b4a0;
const NODE_NAME: u32 = 0x08;
const PARENT: u32 = 0x18;
const LOCAL_ROTATION: u32 = 0x34;
const LOCAL_TRANSLATION: u32 = 0x58;
const WORLD_ROTATION: u32 = 0x68;
const WORLD_TRANSLATION: u32 = 0x8c;
/// `NiUpdateData`'s constructor (time, flags) and `NiAVObject::Update` (as `KnockExplosion` updates
/// a body).
const UPDATE_DATA: u32 = 0x0043_d410;
const NODE_UPDATE: u32 = 0x00a5_9c60;
const LOOKUP_FORM: u32 = 0x0048_39c0;
const FORM_ID: u32 = 0x0c;
const PROCESS: u32 = 0x68;
const PROCESS_LEVEL: u32 = 0x28;
const KNOCK_STATE: u32 = 0x13c;
const LIFE_STATE: u32 = 0x108;
const MAX_NODES: usize = 400;
/// `TESObjectREFR::GetScale`, a reference's position; how far its AI may move it before the steps
/// give way (units).
const GET_SCALE: u32 = 0x0056_7400;
const REFR_POSITION: u32 = 0x30;
const MOVED_AWAY: f32 = 30.0;

fn read3(e: &Engine, at: u32) -> V3 {
    [0, 4, 8].map(|o| e.mem.f32(at + o))
}
fn read9(e: &Engine, at: u32) -> M3 {
    let mut m = [0.0; 9];
    for (i, v) in m.iter_mut().enumerate() {
        *v = e.mem.f32(at + 4 * i as u32);
    }
    m
}
fn write3(e: &mut Engine, at: u32, v: V3) {
    for (i, x) in v.into_iter().enumerate() {
        e.mem.set_f32(at + 4 * i as u32, x);
    }
}
fn write9(e: &mut Engine, at: u32, m: &M3) {
    for (i, x) in m.iter().enumerate() {
        e.mem.set_f32(at + 4 * i as u32, *x);
    }
}

/// The actor's rig bones found in its 3D (by name, [`BONES`]).
fn find_bones(e: &mut Engine, root: u32) -> Vec<Bone> {
    let mut found: Vec<Bone> = Vec::new();
    let mut stack = vec![root];
    let mut seen = 0;
    while let Some(node) = stack.pop() {
        if node == 0 || seen >= MAX_NODES {
            continue;
        }
        seen += 1;
        let name = e.mem.u32(node + NODE_NAME);
        if name != 0 {
            let text = e.mem.cstr(name);
            if let Some(&(_, role)) = BONES
                .iter()
                .find(|(n, _)| text.eq_ignore_ascii_case(n.as_bytes()))
            {
                let side = if text.windows(3).any(|w| w.eq_ignore_ascii_case(b" l ")) {
                    0
                } else if text.windows(3).any(|w| w.eq_ignore_ascii_case(b" r ")) {
                    1
                } else {
                    2
                };
                found.push(Bone {
                    node,
                    role,
                    side,
                    spring: Spring::REST,
                    pose: None,
                    written: None,
                });
            }
        }
        let children = e.vcall(node, 0xc, &[]).u32();
        if children != 0 {
            let count = e.call(CHILD_COUNT, &args![children]).u32().min(64);
            for i in 0..count {
                stack.push(e.call(CHILD_AT, &args![children, i]).u32());
            }
        }
    }
    // Parents before children (the pelvis, the spine up, then the limbs), as [`BONES`] lists them.
    found.sort_by_key(|b| BONES.iter().position(|(_, r)| *r == b.role).unwrap_or(99));
    found
}

/// Whether `actor` can flinch now (module documentation).
fn can_flinch(e: &mut Engine, actor: u32) -> bool {
    let process = e.mem.u32(actor + PROCESS);
    process != 0
        && e.mem.u8(process + PROCESS_LEVEL) == 0
        && e.mem.u8(process + KNOCK_STATE) == 0
        && !matches!(e.mem.u32(actor + LIFE_STATE), 1 | 2)
        && !crate::knockdown::in_get_up(actor)
        && e.call(ACTOR_3D, &args![actor]).u32() != 0
}

/// The actor's rig made (its bones found) when it has none for its 3D; false when it has no rig
/// bones.
fn ensure_rig(e: &mut Engine, actor: u32) -> bool {
    let root = e.call(ACTOR_3D, &args![actor]).u32();
    if root == 0 {
        return false;
    }
    if RIGS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|r| r.actor == actor && r.root == root)
    {
        return true;
    }
    let bones = find_bones(e, root);
    if bones.is_empty() {
        return false;
    }
    let id = e.mem.u32(actor + FORM_ID);
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    rigs.retain(|r| r.actor != actor);
    rigs.push(Rig {
        actor,
        id,
        root,
        bones,
        drop: 0.0,
        drop_speed: 0.0,
        since: Instant::now(),
        balance: None,
        shift: [0.0; 3],
        limbs: crate::balance::Limbs::WHOLE,
    });
    true
}

/// A hit below the topple speed (from `crate::knockdown`): `actor` takes the momentum `p` (N s,
/// this hit's) on its `mass` (kg) at `point` along `direction` (unit) (module documentation).
pub fn hit(e: &mut Engine, actor: u32, p: f32, mass: f32, point: V3, direction: V3) {
    let strength = crate::gunplay::tuning().hit_reactions;
    if strength <= 0 || !crate::gunplay::is_on() || !(p > 0.0 && mass > 0.0) {
        return;
    }
    if !can_flinch(e, actor) {
        return;
    }
    let Some(direction) = unit(direction) else {
        return;
    };
    let v = p / mass * strength as f32 / 100.0;
    if !ensure_rig(e, actor) {
        return;
    }
    let root = e.call(ACTOR_3D, &args![actor]).u32();
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(index) = rigs.iter().position(|r| r.actor == actor && r.root == root) else {
        return;
    };
    let rig = &mut rigs[index];
    rig.since = Instant::now();
    let spread = spread_of(v);
    // The struck bone: the rig's bone nearest the impact.
    let positions: Vec<V3> = rig
        .bones
        .iter()
        .map(|b| read3(e, b.node + WORLD_TRANSLATION))
        .collect();
    let struck = (0..positions.len())
        .min_by(|&a, &b| {
            length(sub(positions[a], point))
                .partial_cmp(&length(sub(positions[b], point)))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);
    let struck_role = rig.bones[struck].role;
    let mut touched = 0;
    let mut spine_kick: Option<V3> = None;
    for (i, bone) in rig.bones.iter_mut().enumerate() {
        let distance = length(sub(positions[i], point)) / UNITS_PER_METRE;
        let w = weight(bone.role, distance, spread);
        if w < 0.02 {
            continue;
        }
        // The lever from the bone's joint to the impact: the kick turns the bone about the axis
        // across it and the hit (a hit at the joint itself turns it about its own length).
        let lever = sub(point, positions[i]);
        let lever = if length(lever) < 5.0 {
            [0.0, 0.0, 1.0]
        } else {
            lever
        };
        let Some(axis) = unit(cross(lever, direction)) else {
            continue;
        };
        let mut kick = scale(axis, KICK * v * w);
        if bone.role == Role::Head {
            // The head lags the spine: kicked against it, it snaps after.
            if let Some(s) = spine_kick {
                kick = add(kick, scale(s, -HEAD_LAG));
            }
        }
        if bone.role == Role::Spine {
            spine_kick = Some(kick);
        }
        bone.spring.velocity = add(bone.spring.velocity, kick);
        bone.spring.dip = (bone.spring.dip + MUSCLE_DIP * w.min(1.0)).min(0.9);
        touched += 1;
    }
    // A leg hit: the knee buckles (the thigh and the calf of that side bend forward, the axis across
    // the leg) and the pelvis drops.
    if matches!(struck_role, Role::Thigh | Role::Calf) {
        let side = rig.bones[struck].node;
        let thigh_or_calf: Vec<usize> = rig
            .bones
            .iter()
            .enumerate()
            .filter(|(_, b)| matches!(b.role, Role::Thigh | Role::Calf))
            .filter(|(i, _)| {
                length(sub(positions[*i], positions[struck])) < 60.0 || rig.bones[*i].node == side
            })
            .map(|(i, _)| i)
            .collect();
        let across = unit(cross([0.0, 0.0, 1.0], direction)).unwrap_or([1.0, 0.0, 0.0]);
        for i in thigh_or_calf {
            let sign = if rig.bones[i].role == Role::Calf {
                -1.0
            } else {
                1.0
            };
            let b = &mut rig.bones[i];
            b.spring.velocity = add(b.spring.velocity, scale(across, sign * KNEE_BUCKLE * v));
        }
        rig.drop_speed -= PELVIS_DROP * v;
    }
    let who = describe(e, actor);
    note(format!(
        "{who}: flinch at {}: {p:.1} N s on {mass:.0} kg -> {v:.2} m/s (Hit Reactions {strength}%), \
         spread {spread:.2} m, {touched} of {} bones",
        BONES
            .iter()
            .find(|(_, r)| *r == struck_role)
            .map_or("a bone", |b| b.0),
        rig.bones.len()
    ));
}

/// Once a frame, at the draw's set-up (from `crate::gunplay`'s `FIRST_PERSON_SETUP` wrapper): every
/// flinching actor's springs stepped and its bones posed (module documentation).
pub fn apply(e: &mut Engine) {
    let dt = {
        let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
        let dt = last.map_or(0.0, |t| t.elapsed().as_secs_f32());
        *last = Some(Instant::now());
        dt.clamp(0.0, MAX_DT)
    };
    let rigs: Vec<Rig> = std::mem::take(&mut *RIGS.lock().unwrap_or_else(|p| p.into_inner()));
    if rigs.is_empty() {
        return;
    }
    let mut keep = Vec::new();
    for mut rig in rigs {
        if !(rig.id != 0 && e.call(LOOKUP_FORM, &args![rig.id]).u32() == rig.actor) {
            continue; // gone (a load, an unload): nothing of it is touched
        }
        let root = e.call(ACTOR_3D, &args![rig.actor]).u32();
        if root != rig.root {
            continue; // its 3D was made again: the old nodes are not ours
        }
        let flinching = can_flinch(e, rig.actor);
        // The pose under the offset: what the animation wrote, or (when it did not write the bone
        // again) the pose the last offset was written over.
        for bone in &mut rig.bones {
            let now = (
                read9(e, bone.node + LOCAL_ROTATION),
                read3(e, bone.node + LOCAL_TRANSLATION),
            );
            let base = match (bone.pose, bone.written) {
                (Some(pose), Some(w)) if w == now => pose,
                _ => now,
            };
            bone.pose = Some(base);
        }
        let mut overrides: Vec<Option<M3>> = vec![None; rig.bones.len()];
        if flinching {
            for bone in &mut rig.bones {
                bone.spring.step(bone.role, dt);
            }
            overrides = balance_frame(e, &mut rig, dt);
            let (f, zeta) = Role::Pelvis.spring();
            let w0 = std::f32::consts::TAU * f;
            rig.drop_speed += (-w0 * w0 * rig.drop - 2.0 * zeta * w0 * rig.drop_speed) * dt;
            rig.drop = (rig.drop + rig.drop_speed * dt).clamp(-PELVIS_DROP_MAX, 0.0);
        } else {
            for bone in &mut rig.bones {
                bone.spring = Spring::REST;
            }
            rig.drop = 0.0;
            rig.drop_speed = 0.0;
            rig.balance = None;
            rig.shift = [0.0; 3];
        }
        let resting = rig.bones.iter().all(|b| b.spring.at_rest())
            && rig.drop.abs() < 1e-3
            && rig.balance.is_none();
        let shift = rig.shift;
        for (index, bone) in rig.bones.iter_mut().enumerate() {
            let (pose_rot, pose_t) = bone.pose.unwrap_or_default();
            let (rot, t) = if resting {
                (pose_rot, pose_t)
            } else {
                let parent = e.mem.u32(bone.node + PARENT);
                let p = if parent == 0 {
                    [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
                } else {
                    read9(e, parent + WORLD_ROTATION)
                };
                // The world-space offset carried into the bone's frame: P^T R P L.
                let spring = rotation(bone.spring.angle);
                let r = match overrides[index] {
                    Some(o) => mul(&o, &spring),
                    None => spring,
                };
                let rot = mul(&mul(&mul(&transpose(&p), &r), &p), &pose_rot);
                let t = if bone.role == Role::Pelvis {
                    add(
                        pose_t,
                        apply_m(
                            &transpose(&p),
                            add([0.0, 0.0, rig.drop * UNITS_PER_METRE], shift),
                        ),
                    )
                } else {
                    pose_t
                };
                (rot, t)
            };
            if rot.iter().all(|x| x.is_finite()) && t.iter().all(|x| x.is_finite()) {
                write9(e, bone.node + LOCAL_ROTATION, &rot);
                write3(e, bone.node + LOCAL_TRANSLATION, t);
                bone.written = Some((rot, t));
            }
        }
        update(e, root);
        if resting && rig.since.elapsed().as_secs_f32() > 0.2 {
            continue; // back in its pose: done
        }
        keep.push(rig);
    }
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    // Rigs a hit made while this ran come after.
    let fresh = std::mem::take(&mut *rigs);
    *rigs = keep;
    for f in fresh {
        rigs.retain(|r| r.actor != f.actor);
        rigs.push(f);
    }
}

/// The rig's bone of `role` on `side`, its index.
fn bone_of(rig: &Rig, role: Role, side: u8) -> Option<usize> {
    rig.bones
        .iter()
        .position(|b| b.role == role && (side == 2 || b.side == side))
}

fn actor_scale(e: &mut Engine, actor: u32) -> f32 {
    let s = e.call(GET_SCALE, &args![actor]).f64() as f32;
    if s.is_finite() {
        s.clamp(0.25, 4.0)
    } else {
        1.0
    }
}

/// Stage 2's frame for a rig with catch steps under way ([`crate::balance`]): the steps advanced
/// (a landed foot moves the reference with the feet), the pelvis carried over the feet, the legs
/// placed by two-bone IK on their targets, the arms out. Answers each bone's world-space rotation
/// on top of its spring's.
fn balance_frame(e: &mut Engine, rig: &mut Rig, dt: f32) -> Vec<Option<M3>> {
    use crate::balance::{between, two_bone, Outcome};
    let mut out: Vec<Option<M3>> = vec![None; rig.bones.len()];
    let Some(mut s) = rig.balance.take() else {
        rig.shift = [0.0; 3];
        return out;
    };
    let actor = rig.actor;
    let scale_ = actor_scale(e, actor);
    let reference = read3(e, actor + REFR_POSITION);
    if !s.recovering && length(sub(reference, s.reference)) > MOVED_AWAY {
        s.recovering = true;
        note(format!(
            "{}: steps end: its AI moved it ({:.0} units)",
            describe(e, actor),
            length(sub(reference, s.reference))
        ));
    }
    // A ledge: the foot goes over it, and the body falls into its ragdoll.
    if s.outcome == Outcome::Ledge
        && s.current + 1 >= s.steps.len()
        && s.steps.last().is_some_and(|l| s.t > l.time * 0.5)
    {
        let pelvis = bone_of(rig, Role::Pelvis, 2)
            .map(|i| read3(e, rig.bones[i].node + WORLD_TRANSLATION))
            .unwrap_or(reference);
        note(format!(
            "{}: steps off a ledge: falls ({:.2} m/s)",
            describe(e, actor),
            s.v
        ));
        rig.shift = [0.0; 3];
        crate::knockdown::knock_down(e, actor, s.aggressor, pelvis, s.dir, s.v);
        return out;
    }
    if let Some((foot, at)) = s.advance(dt) {
        // The reference follows the feet: to their middle, on the ground; the pelvis's offset
        // takes the move back.
        let to = s.follow_feet();
        let to = [
            to[0],
            to[1],
            to[2].clamp(reference[2] - 60.0, reference[2] + 60.0),
        ];
        let moved = sub(to, reference);
        crate::knockdown::place(e, actor, to);
        s.reference = to;
        s.shift = sub(s.shift, moved);
        note(format!(
            "{}: {} foot lands at ({:.0}, {:.0}, {:.0}), step {} of {}; the actor follows the feet \
             {:.0} units",
            describe(e, actor),
            if foot == 0 { "left" } else { "right" },
            at[0],
            at[1],
            at[2],
            s.current,
            s.steps.len(),
            length(moved)
        ));
    }
    let pelvis =
        bone_of(rig, Role::Pelvis, 2).map(|i| read3(e, rig.bones[i].node + WORLD_TRANSLATION));
    if let Some(pelvis) = pelvis {
        let target = s.shift_target(pelvis, scale_);
        s.follow(target, dt);
    }
    let shift = s.shift;
    let feet = s.feet(scale_);
    for side in 0..2u8 {
        let (Some(t), Some(c), Some(f)) = (
            bone_of(rig, Role::Thigh, side),
            bone_of(rig, Role::Calf, side),
            bone_of(rig, Role::Foot, side),
        ) else {
            continue;
        };
        let hip0 = read3(e, rig.bones[t].node + WORLD_TRANSLATION);
        let knee0 = read3(e, rig.bones[c].node + WORLD_TRANSLATION);
        let ankle0 = read3(e, rig.bones[f].node + WORLD_TRANSLATION);
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
        // The foot kept level: its world rotation as the animation had it.
        let level = mul(&transpose(&m), &transpose(&r1));
        out[t] = Some(r1);
        out[c] = Some(m);
        out[f] = Some(level);
    }
    // The arms out, raised from the body.
    let spine = bone_of(rig, Role::Spine, 2)
        .map(|i| read3(e, rig.bones[i].node + WORLD_TRANSLATION))
        .or(pelvis);
    for side in 0..2u8 {
        if !rig.limbs.arm(side as usize) || s.arms <= 0.0 {
            continue;
        }
        let (Some(u), Some(f), Some(spine)) = (
            bone_of(rig, Role::UpperArm, side),
            bone_of(rig, Role::Forearm, side),
            spine,
        ) else {
            continue;
        };
        let shoulder = read3(e, rig.bones[u].node + WORLD_TRANSLATION);
        let elbow = read3(e, rig.bones[f].node + WORLD_TRANSLATION);
        let outward = [shoulder[0] - spine[0], shoulder[1] - spine[1], 0.0];
        if let Some(axis) = unit(cross(sub(elbow, shoulder), outward)) {
            out[u] = Some(rotation(scale(axis, crate::balance::ARMS_OUT * s.arms)));
        }
    }
    rig.shift = shift;
    if s.done() {
        note(format!(
            "{}: steps done ({:?}): back to its animation",
            describe(e, actor),
            s.outcome
        ));
        rig.shift = [0.0; 3];
    } else {
        rig.balance = Some(s);
    }
    out
}

/// Stage 2's stimulus: a hit below the topple speed (from `crate::knockdown`) pushes the body's
/// centre of mass with `v = p / mass x iBalance %`; past its feet, it takes catch steps
/// ([`crate::balance`]).
pub fn push(e: &mut Engine, actor: u32, p: f32, mass: f32, direction: V3, aggressor: u32) {
    use crate::balance::{classify, plan, Ground, Outcome, Stepping};
    let strength = crate::gunplay::tuning().balance;
    if strength <= 0 || !crate::gunplay::is_on() || !(p > 0.0 && mass > 0.0) {
        return;
    }
    if !can_flinch(e, actor) || !ensure_rig(e, actor) {
        return;
    }
    let Some(dir) = unit([direction[0], direction[1], 0.0]) else {
        return;
    };
    let v = p / mass * strength as f32 / 100.0;
    let scale_ = actor_scale(e, actor);
    let root = e.call(ACTOR_3D, &args![actor]).u32();
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(rig) = rigs.iter_mut().find(|r| r.actor == actor && r.root == root) else {
        return;
    };
    if !rig.limbs.legs() {
        return; // one leg cannot step (stage 3 falls)
    }
    let (Some(pi), Some(lf), Some(rf)) = (
        bone_of(rig, Role::Pelvis, 2),
        bone_of(rig, Role::Foot, 0),
        bone_of(rig, Role::Foot, 1),
    ) else {
        return;
    };
    let pelvis = add(read3(e, rig.bones[pi].node + WORLD_TRANSLATION), rig.shift);
    let (mut feet, carried) = match &rig.balance {
        Some(s) if !s.recovering => (s.planted, s.v * 0.5),
        _ => (
            [
                read3(e, rig.bones[lf].node + WORLD_TRANSLATION),
                read3(e, rig.bones[rf].node + WORLD_TRANSLATION),
            ],
            0.0,
        ),
    };
    let v = v + carried;
    let h = ((pelvis[2] - (feet[0][2] + feet[1][2]) / 2.0) / UNITS_PER_METRE).clamp(0.3, 2.5);
    let planned = plan(pelvis, feet, dir, v, h, scale_);
    let who = describe(e, actor);
    if planned.steps.is_empty() {
        return; // the feet hold it: the flinch alone
    }
    let mut outcome =
        if planned.need > crate::balance::STEP_LENGTH * scale_ * crate::balance::MAX_STEPS as f32 {
            Outcome::Overrun
        } else {
            Outcome::Caught
        };
    let start = feet;
    let mut steps = Vec::new();
    let mut said = Vec::new();
    drop(rigs);
    // Each foot's ankle over its ground (the navmesh's height is the ground's; the foot bone is
    // the ankle): kept for its steps.
    let ankle: [f32; 2] = [0, 1].map(|f| {
        crate::knockdown::navmesh_point(e, actor, start[f])
            .map(|g| start[f][2] - g[2])
            .filter(|h| (0.0..=25.0).contains(h))
            .unwrap_or(8.0 * scale_)
    });
    for (foot, to) in planned.steps {
        let from = feet[foot];
        let ground = classify(to, from[2], crate::knockdown::navmesh_point(e, actor, to));
        match ground {
            Ground::Floor(z) => {
                let to = [to[0], to[1], z + ankle[foot]];
                steps.push((foot, to));
                feet[foot] = to;
            }
            Ground::Ledge => {
                steps.push((foot, [to[0], to[1], from[2]]));
                outcome = Outcome::Ledge;
                said.push("a ledge");
                break;
            }
            Ground::Blocked => {
                // A wall or an obstacle: as far as there is room.
                let mut placed = false;
                for k in [0.66f32, 0.33] {
                    let short = add(from, scale(sub(to, from), k));
                    if let Ground::Floor(z) = classify(
                        short,
                        from[2],
                        crate::knockdown::navmesh_point(e, actor, short),
                    ) {
                        let short = [short[0], short[1], z + ankle[foot]];
                        steps.push((foot, short));
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
    if steps.is_empty() {
        note(format!(
            "{who}: pushed {v:.2} m/s past its feet ({:.2} m): against a wall, it catches itself",
            planned.need
        ));
        return;
    }
    let reference = read3(e, actor + REFR_POSITION);
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
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(rig) = rigs.iter_mut().find(|r| r.actor == actor && r.root == root) else {
        return;
    };
    let stepping = Stepping::new(dir, v, aggressor, start, steps, scale_, reference, outcome);
    rig.balance = Some(stepping);
    rig.since = Instant::now();
    drop(rigs);
    note(format!(
        "{who}: balance: pushed {v:.2} m/s along ({:.2}, {:.2}), capture point {:.2} m past its feet: \
         {count} step(s) ({} m){}; {:?}",
        dir[0],
        dir[1],
        planned.need,
        lengths.join(", "),
        if said.is_empty() {
            String::new()
        } else {
            format!(", {}", said.join(", "))
        },
        outcome
    ));
}

/// The actor's 3D's world transforms brought up to its bones' local ones (`NiAVObject::Update`
/// with an update data of no time and no controllers).
fn update(e: &mut Engine, root: u32) {
    e.with_stack(16, |e, data| {
        e.call(UPDATE_DATA, &args![data.addr(), 0.0f32, 0u32, 0u32]);
        e.call(NODE_UPDATE, &args![root, data.addr()]);
    });
}

/// Everything dropped (a load, a new game, the main menu): the nodes go with the game.
pub fn clear() {
    RIGS.lock().unwrap_or_else(|p| p.into_inner()).clear();
    *LAST.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

fn describe(e: &Engine, actor: u32) -> String {
    format!("Hit reaction: {:08X}", e.mem.u32(actor + FORM_ID))
}

/// The log lines, at most 20 a second.
fn note(line: String) {
    static N: Mutex<(Option<Instant>, u32)> = Mutex::new((None, 0));
    let mut n = N.lock().unwrap_or_else(|p| p.into_inner());
    if n.0.map_or(true, |t| t.elapsed().as_secs() >= 1) {
        *n = (Some(Instant::now()), 0);
    }
    if n.1 < 20 {
        n.1 += 1;
        eprintln!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kicked bone swings out and springs back past its pose (an overshoot), then comes to rest.
    #[test]
    fn a_kicked_bone_springs_back_with_an_overshoot() {
        let mut s = Spring::REST;
        s.velocity = [0.0, 0.0, 2.0];
        let mut peak = 0.0f32;
        let mut least = 0.0f32;
        for _ in 0..240 {
            s.step(Role::Spine, 1.0 / 60.0);
            peak = peak.max(s.angle[2]);
            least = least.min(s.angle[2]);
        }
        assert!(peak > 0.05 && peak <= Role::Spine.limit(), "{peak}");
        assert!(least < -0.005, "an overshoot: {least}");
        assert!(s.at_rest(), "{s:?}");
    }

    /// A lowered muscle lets the bone swing farther; a hard kick stops at the joint's limit.
    #[test]
    fn muscles_and_limits() {
        let swing = |dip: f32| {
            let mut s = Spring {
                velocity: [1.0, 0.0, 0.0],
                dip,
                ..Spring::REST
            };
            (0..30)
                .map(|_| {
                    s.step(Role::Spine, 1.0 / 60.0);
                    s.angle[0]
                })
                .fold(0.0f32, f32::max)
        };
        assert!(swing(0.6) > swing(0.0));
        let mut s = Spring {
            velocity: [100.0, 0.0, 0.0],
            ..Spring::REST
        };
        s.step(Role::Head, 1.0 / 60.0);
        assert!((length(s.angle) - Role::Head.limit()).abs() < 1e-5);
    }

    /// A pistol round's kick stays near the hit; a .50's or a sledgehammer's spreads through the body.
    #[test]
    fn bigger_hits_spread_farther() {
        let pistol = spread_of(0.3);
        let fifty = spread_of(1.3);
        assert!(fifty > 2.0 * pistol);
        // A hip 0.5 m from a shoulder hit: little from a pistol round, a good share from a .50.
        assert!(weight(Role::Pelvis, 0.5, pistol) < 0.1);
        assert!(weight(Role::Pelvis, 0.5, fifty) > 0.15);
        assert_eq!(spread_of(100.0), SPREAD_MAX);
        assert!(weight(Role::UpperArm, 0.0, pistol) > weight(Role::Pelvis, 0.0, pistol));
    }

    /// The rotation of a rotation vector: a quarter turn about z takes x to y; none is the identity.
    #[test]
    fn rotations() {
        let r = rotation([0.0, 0.0, std::f32::consts::FRAC_PI_2]);
        let v = apply_m(&r, [1.0, 0.0, 0.0]);
        assert!((v[0]).abs() < 1e-5 && (v[1] - 1.0).abs() < 1e-5);
        assert_eq!(
            rotation([0.0; 3]),
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        );
        let p = rotation([0.3, 0.2, 0.1]);
        let id = mul(&transpose(&p), &p);
        for (i, x) in id.iter().enumerate() {
            let want = if i % 4 == 0 { 1.0 } else { 0.0 };
            assert!((x - want).abs() < 1e-5);
        }
    }
}
