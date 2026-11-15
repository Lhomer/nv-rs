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
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// The pure parts (the roles, the springs, the spread, the vector helpers, stage 3's timings) live
/// in `crates/ragdoll-core`, shared with `tools/ragdoll-lab`.
pub use ragdoll_core::flinch::*;

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
    /// Stage 3: hands braced on a wall, and the fall into the ragdoll under way.
    pub brace: Option<Brace>,
    pub fall: Option<Fall>,
}

/// Hands reaching to a wall along `dir` (stage 3), `t` seconds in.
#[derive(Clone, Copy, Debug)]
pub struct Brace {
    pub dir: V3,
    pub t: f32,
}

/// The fall into the ragdoll under way (stage 3): along `dir` at `speed` (m/s), for the attacker,
/// from `point`; `lead` seconds of the posed body going over (with the arms windmilling at a
/// ledge) before the ragdoll takes it.
#[derive(Clone, Copy, Debug)]
pub struct Fall {
    pub dir: V3,
    pub speed: f32,
    pub aggressor: u32,
    pub point: V3,
    pub t: f32,
    pub lead: f32,
    pub windmill: bool,
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

/// Seconds on a monotonic clock (the steppings' start and end, [`crate::balance::Stepping`]).
pub fn clock() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}
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
        limbs: crate::balance::limbs_of(actor),
        brace: None,
        fall: None,
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
        // In its ragdoll (knocked down) or dead: the ragdoll owns the bones now; nothing is
        // written back (that would fight it), the rig just goes.
        let process = e.mem.u32(rig.actor + PROCESS);
        if process == 0
            || e.mem.u8(process + KNOCK_STATE) != 0
            || matches!(e.mem.u32(rig.actor + LIFE_STATE), 1 | 2)
        {
            continue;
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
            brace_frame(e, &mut rig, &mut overrides, dt);
            if fall_frame(e, &mut rig, &mut overrides, dt) {
                continue; // in the ragdoll now: it owns the bones
            }
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
            rig.brace = None;
            rig.fall = None;
        }
        let resting = rig.bones.iter().all(|b| b.spring.at_rest())
            && rig.drop.abs() < 1e-3
            && rig.balance.is_none()
            && rig.brace.is_none()
            && rig.fall.is_none();
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
            "{}: steps off a ledge: windmills, then falls ({:.2} m/s)",
            describe(e, actor),
            s.v
        ));
        rig.fall = Some(Fall {
            dir: s.dir,
            speed: s.v,
            aggressor: s.aggressor,
            point: pelvis,
            t: 0.0,
            lead: LEDGE_LEAD,
            windmill: true,
        });
        rig.shift = s.shift;
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
    if s.recovering && s.outcome == Outcome::Overrun && rig.fall.is_none() {
        // More than its steps could catch: it goes over at the end of them.
        let at = pelvis.unwrap_or(reference);
        note(format!(
            "{}: the steps could not catch it: falls ({:.2} m/s)",
            describe(e, actor),
            s.v * 0.6
        ));
        rig.fall = Some(Fall {
            dir: s.dir,
            speed: s.v * 0.6,
            aggressor: s.aggressor,
            point: at,
            t: 0.0,
            lead: OVERRUN_LEAD,
            windmill: false,
        });
        rig.shift = shift;
        return out;
    }
    if s.done(clock()) {
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
    let one_leg = !rig.limbs.legs();
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
    if one_leg {
        // One leg cannot step: past its foot it goes over.
        rig.fall = Some(Fall {
            dir,
            speed: v,
            aggressor,
            point: pelvis,
            t: 0.0,
            lead: FALL_LEAD,
            windmill: false,
        });
        rig.balance = None;
        drop(rigs);
        note(format!(
            "{who}: on one leg it cannot step: falls ({v:.2} m/s)"
        ));
        return;
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
    let against_wall = outcome == Outcome::Wall;
    if against_wall {
        start_brace(e, actor, root, dir, v, aggressor);
    }
    if steps.is_empty() {
        note(format!(
            "{who}: pushed {v:.2} m/s past its feet ({:.2} m): against a wall, it catches itself \
             with its hands",
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
    let stepping = Stepping::new(
        dir,
        v,
        aggressor,
        start,
        steps,
        scale_,
        reference,
        outcome,
        clock(),
    );
    rig.balance = Some(stepping);
    if against_wall && rig.brace.is_none() {
        rig.brace = Some(Brace { dir, t: 0.0 });
    }
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

/// A wall's brace begun (stage 3): the hands reach to it; past what a brace holds the body
/// crumples against it into its ragdoll.
fn start_brace(e: &mut Engine, actor: u32, root: u32, dir: V3, v: f32, aggressor: u32) {
    let limit = BRACE_HOLDS * crate::gunplay::tuning().balance.max(1) as f32 / 100.0;
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(rig) = rigs.iter_mut().find(|r| r.actor == actor && r.root == root) else {
        return;
    };
    let arms = rig.limbs.arm(0) || rig.limbs.arm(1);
    if arms {
        rig.brace = Some(Brace { dir, t: 0.0 });
    }
    if v > limit || !arms {
        let point = bone_of(rig, Role::Pelvis, 2)
            .map(|i| read3(e, rig.bones[i].node + WORLD_TRANSLATION))
            .unwrap_or([0.0; 3]);
        rig.fall = Some(Fall {
            dir,
            speed: v * 0.5,
            aggressor,
            point,
            t: 0.0,
            lead: BRACE_REACH + 0.1,
            windmill: false,
        });
    }
    drop(rigs);
    note(format!(
        "{}: braces against a wall with {}{}",
        describe(e, actor),
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

/// The arm of `side` reaching its hand to `target` (two-bone IK of the upper arm and the forearm),
/// blended by `weight`: the upper arm's and the forearm's rotations.
fn reach_arm(
    e: &Engine,
    rig: &Rig,
    side: u8,
    target: V3,
    weight: f32,
) -> Option<(usize, M3, usize, M3)> {
    use crate::balance::{between, two_bone};
    let u = bone_of(rig, Role::UpperArm, side)?;
    let f = bone_of(rig, Role::Forearm, side)?;
    let h = bone_of(rig, Role::Hand, side)?;
    let shoulder = read3(e, rig.bones[u].node + WORLD_TRANSLATION);
    let elbow0 = read3(e, rig.bones[f].node + WORLD_TRANSLATION);
    let hand0 = read3(e, rig.bones[h].node + WORLD_TRANSLATION);
    let (l1, l2) = (length(sub(elbow0, shoulder)), length(sub(hand0, elbow0)));
    if l1 < 1.0 || l2 < 1.0 {
        return None;
    }
    let goal = add(hand0, scale(sub(target, hand0), weight.clamp(0.0, 1.0)));
    let pole = add(elbow0, [0.0, 0.0, -20.0]);
    let (elbow, reached) = two_bone(shoulder, goal, l1, l2, pole)?;
    let r1 = rotation(between(sub(elbow0, shoulder), sub(elbow, shoulder)));
    let r2 = rotation(between(
        apply_m(&r1, sub(hand0, elbow0)),
        sub(reached, elbow),
    ));
    Some((u, r1, f, mul(&mul(&transpose(&r1), &r2), &r1)))
}

/// Stage 3's wall brace for a frame: both hands (the arms it has) reach to the wall ahead along
/// the push, at the shoulders' height.
fn brace_frame(e: &mut Engine, rig: &mut Rig, out: &mut [Option<M3>], dt: f32) {
    let Some(mut b) = rig.brace.take() else {
        return;
    };
    b.t += dt;
    let Some(w) = brace_weight(b.t) else {
        note(format!("{}: lets go of the wall", describe(e, rig.actor)));
        return;
    };
    for side in 0..2u8 {
        if !rig.limbs.arm(side as usize) {
            continue;
        }
        let Some(u) = bone_of(rig, Role::UpperArm, side) else {
            continue;
        };
        let shoulder = read3(e, rig.bones[u].node + WORLD_TRANSLATION);
        let target = add(
            shoulder,
            add(scale(b.dir, 0.55 * UNITS_PER_METRE), [0.0, 0.0, -8.0]),
        );
        if let Some((ui, ur, fi, fr)) = reach_arm(e, rig, side, target, w) {
            out[ui] = Some(ur);
            out[fi] = Some(fr);
        }
    }
    rig.brace = Some(b);
}

/// Stage 3's fall for a frame: the posed body goes over along the fall (torso and knees, the
/// pelvis following; the arms windmilling at a ledge) for its lead, then the ragdoll takes it with
/// the fall's speed (`crate::knockdown::knock_down`). True when the ragdoll has it.
fn fall_frame(e: &mut Engine, rig: &mut Rig, out: &mut [Option<M3>], dt: f32) -> bool {
    let Some(mut f) = rig.fall.take() else {
        return false;
    };
    rig.balance = None;
    f.t += dt;
    let k = (f.t / f.lead.max(1e-3)).clamp(0.0, 1.0);
    let across = unit(cross([0.0, 0.0, 1.0], f.dir)).unwrap_or([1.0, 0.0, 0.0]);
    // The torso goes over (a windmilling body leans back against it), the knees give.
    let lean = if f.windmill { -0.3 } else { FALL_LEAN * k };
    if let Some(i) = bone_of(rig, Role::Spine, 2) {
        out[i] = Some(rotation(scale(across, lean)));
    }
    for side in 0..2u8 {
        if let Some(t) = bone_of(rig, Role::Thigh, side) {
            out[t] = Some(rotation(scale(across, FALL_KNEES * k)));
        }
        if let Some(c) = bone_of(rig, Role::Calf, side) {
            out[c] = Some(rotation(scale(across, -FALL_KNEES * k)));
        }
        if f.windmill && rig.limbs.arm(side as usize) {
            if let Some(u) = bone_of(rig, Role::UpperArm, side) {
                let phase = if side == 0 { 0.0 } else { std::f32::consts::PI };
                let a = WINDMILL_SWING * (std::f32::consts::TAU * WINDMILL_HZ * f.t + phase).sin();
                out[u] = Some(rotation(scale(across, a)));
            }
        }
    }
    let go = add(
        rig.shift,
        scale(f.dir, (f.speed * dt * UNITS_PER_METRE).min(FALL_SHIFT)),
    );
    rig.shift = if length(go) <= FALL_SHIFT * 2.0 {
        go
    } else {
        rig.shift
    };
    if f.t < f.lead {
        rig.fall = Some(f);
        return false;
    }
    note(format!(
        "{}: into its ragdoll after {:.2} s of going over{} ({:.2} m/s along ({:.2}, {:.2}))",
        describe(e, rig.actor),
        f.t,
        if f.windmill { ", windmilling" } else { "" },
        f.speed,
        f.dir[0],
        f.dir[1]
    ));
    crate::knockdown::knock_down(e, rig.actor, f.aggressor, f.point, f.dir, f.speed);
    true
}

/// Stage 3's entry for a knockdown (from `crate::knockdown`): instead of the hard switch, the
/// posed body goes over for [`FALL_LEAD`] and then falls into its ragdoll. False when it cannot
/// (no rig, the layer off): the caller knocks it down at once.
pub fn fall(
    e: &mut Engine,
    actor: u32,
    aggressor: u32,
    point: V3,
    direction: V3,
    speed: f32,
) -> bool {
    let t = crate::gunplay::tuning();
    if !crate::gunplay::is_on() || (t.hit_reactions <= 0 && t.balance <= 0) {
        return false;
    }
    if !can_flinch(e, actor) || !ensure_rig(e, actor) {
        return false;
    }
    let root = e.call(ACTOR_3D, &args![actor]).u32();
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(rig) = rigs.iter_mut().find(|r| r.actor == actor && r.root == root) else {
        return false;
    };
    if rig.fall.is_some() {
        return true;
    }
    let dir = unit([direction[0], direction[1], 0.0]).unwrap_or([0.0, 1.0, 0.0]);
    rig.fall = Some(Fall {
        dir,
        speed,
        aggressor,
        point,
        t: 0.0,
        lead: FALL_LEAD,
        windmill: false,
    });
    rig.since = Instant::now();
    true
}

/// Whether `actor` is going over into its ragdoll now (its hits wait for the ragdoll).
pub fn falling(actor: u32) -> bool {
    RIGS.lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|r| r.actor == actor && r.fall.is_some())
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
