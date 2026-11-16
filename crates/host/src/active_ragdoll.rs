//! Active ragdoll, stage 1: the physics flinch (Patrick, 2026-11-15: a GTA IV / Red Dead style
//! active ragdoll, in stages; stage 2 balance and procedural stepping, stage 3 bracing and falling
//! into the ragdoll come later). Part of the weighty combat row; `[nv-rs Gunplay] iHitReactions`
//! (percent, 0 = off; Settings > Gameplay "Hit Reactions"). **Not 1:1 with FalloutNV.exe.**
//!
//! **Now a physical body** (Patrick, 2026-11-15: toward Euphoria; it supersedes the springs
//! described below, whose tables still give each joint its motor's frequency, its share of a kick
//! and its room). A hit makes the actor's rig a physical articulated body for a second or two
//! (`ragdoll_core::active`): each bone a capsule, every joint a PD motor tracking the animation's
//! pose (soft right after the hit, eased back to full), the hit's momentum shared among the bones
//! near it and reaching each after its distance (it travels up and down the chain), the limbs'
//! weight carried, the pelvis held by a support that follows the capture-point steps
//! ([`crate::balance`]), the feet pinned where they stand or swing; balance comes from those, not
//! from scripted poses (no arms out, no scripted lean: going over is the support fading). The body
//! is made the frame after the hit from two frames of the pose (its motion: nothing jumps), each
//! frame its bones are written as offsets over the animation (`A(p) S(p)^T S(b) A(b)^T`, the
//! same layer and the same ragdoll-controller seam as before), and after [`active::ACTIVE_FOR`]
//! it blends back to the animation over [`active::BLEND_BACK`], eased. When it falls into its
//! ragdoll, the ragdoll goes on with the body's motion ([`body_velocities`]). Tuned in
//! `tools/ragdoll-lab`, which runs the same body.
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
//! **One write for every actor: our own controller** (Patrick, 2026-11-15). The standing body's
//! pose is written once a frame straight onto the skeleton's bones, at the draw's set-up, right
//! after the game animated them: absolute locals over the animated pose
//! (`ragdoll_core::pose`), the same for armed, attacking and unarmed actors. Nothing is written
//! through the game's ragdoll-controller pass (`bhkRagdollController::DoRagdollAnim`, `00c7d630`,
//! from `00888970` on the AI thread): 779cd5b to f4c644e wrote there first, for every actor whose
//! controller had foot IK, grab IK or feedback on (the log called them "armed or attacking", but
//! an unarmed actor with foot IK took it too: Patrick's unarmed Powder Ganger did), and that is
//! where the stretching and the freeze were. When the pass runs after our write it reads our
//! pose and writes it back (a hair off, which the write model takes as still holding it); when it
//! runs before, our write comes after. An armed actor keeps its aim (its arms track their world
//! rotation, firmer); a stagger, a catch step or a fall interrupts an attack (`Actor::StopAttack`,
//! `008a8e50`, [`interrupt_attack`]). Every skipped reaction is logged with its reason
//! ([`blocked`]). The world is probed with the game's own ray casts (`jip::commands::
//! world_ray_cast`: the floor under each step, a ledge, the wall for a brace), the navmesh as the
//! fallback, at the hit on the main thread. Havok's ragdoll takes the body only for the full
//! limp fall (`crate::knockdown`, starting from our bones' velocities); corpses, saves, looting
//! and dragging stay Havok's.
//!
//! **The arms** (Patrick, 2026-11-15: they snapped and flailed): softer, well-damped arm springs,
//! every joint's turn held to a person's speed ([`Role::spin`]), and the poses the arms are given
//! (arms out while stepping, the wall brace's reach, the ledge's windmill, now at a person's pace)
//! followed with a lag and a little follow-through ([`Follow`]) instead of jumps.
//!
//! **Never:** the player, a dead actor, one knocked down or getting up (`crate::knockdown`), a
//! companion unless `bKnockdownCompanions`, an actor below the middle-high process (no animated
//! 3D).
//!
//! **For stage 2 and 3:** [`Rig`] (the skeleton's named bones), [`Spring`] (a bone's offset state)
//! and [`apply`] (the pose layer) are what balance and bracing drive too; [`hit`] is the stimulus
//! entry, and stage 3's fall into the ragdoll goes through `crate::knockdown::knock_down`.

use crate::room_events;
use engine::prelude::*;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use ragdoll_core::brace::{self as hands, Hand, Surface};
use ragdoll_core::condition::{self, Condition};
/// The pure parts (the roles, the springs, the spread, the vector helpers, stage 3's timings) live
/// in `crates/ragdoll-core`, shared with `tools/ragdoll-lab`.
pub use ragdoll_core::flinch::*;
use ragdoll_core::{active, obstacle, pose};

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
    /// Its weapon drawn at the last hit, and how much of its aim it keeps ([`hold_aim`]).
    pub armed: bool,
    pub aim_hold: f32,
    /// The physical body while it reacts (`ragdoll_core::active`), the body's mass (kg), its last
    /// hit, its hits waiting for the body (made the frame after, from two frames of the pose:
    /// its motion), the pose the frame before, the feet's targets between steps (and since when
    /// they go back to the animation's), the steps' last targets.
    pub body: Option<active::Body>,
    pub mass: f32,
    pub hit_at: Instant,
    pub pending: Vec<Hit>,
    pub before: Option<Vec<(M3, V3)>>,
    pub planted: Option<([V3; 2], Instant)>,
    pub held: Vec<Option<M3>>,
    /// The brace's hands: each free arm's wrist this frame (before the brace's IK: where a
    /// released hand goes back to), and a planted hand's shown rotation, held as it touched.
    pub free_wrist: [Option<V3>; 2],
    pub hand_lock: [Option<M3>; 2],
    /// The grip hold ([`home_of`]): a two-handed weapon's support hand, back on its grip after a
    /// brace, kept there while the body is physical.
    pub grip_hold: bool,
}

/// A hit waiting for the physical body (`ragdoll_core::active::Body::hit`'s words; game units).
#[derive(Clone, Debug)]
pub struct Hit {
    pub p: f32,
    pub dir: V3,
    pub at: V3,
    pub spread: f32,
    pub dips: Vec<f32>,
    pub takes: Vec<f32>,
}

/// Hands reaching to a wall along `dir` (stage 3), `t` seconds in.
#[derive(Clone, Copy, Debug)]
pub struct Brace {
    pub dir: V3,
    pub t: f32,
    /// How far ahead the obstacle is (game units; the game's ray casts), if found.
    pub wall: Option<f32>,
    /// A waist-high obstacle's top (world z, game units): the hands go on it.
    pub top: Option<f32>,
    /// What the hands meet, and each bracing hand (`ragdoll_core::brace`; none: that arm does not
    /// brace: missing, or an armed actor's weapon arm keeping its aim).
    pub surface: Option<Surface>,
    pub hands: [Option<Hand>; 2],
}

impl Brace {
    /// The brace's weight on an arm's motor targets (how far its reach is in, 0 to 1).
    pub fn weight(&self, side: usize) -> f32 {
        self.hands[side].map_or(0.0, |h| h.reach_weight(self.t))
    }

    /// Whether an armed actor's weapon arm stays on its aim (it does not brace).
    pub fn aims(&self, side: usize, armed: bool) -> bool {
        armed && side == hands::WEAPON_SIDE as usize && self.hands[side].is_none()
    }
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
    /// Folding over a waist-high obstacle: its top (world z, game units).
    pub fold: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct Bone {
    pub node: u32,
    pub role: Role,
    /// 0 left, 1 right, 2 the middle.
    pub side: u8,
    pub spring: Spring,
    /// The animated pose (local rotation, local translation) and what was written over it.
    pub pose: Option<(M3, V3)>,
    pub written: Option<(M3, V3)>,
    /// The rig bone it hangs from, and (an arm's) its shown pose offset following the poses it is
    /// given ([`Follow`]).
    pub parent: Option<usize>,
    pub follow: Follow,
    /// The nodes between its rig parent and it (top down; none of them written), and (without a
    /// rig parent) the node it hangs from: the animated world under it is worked out from them
    /// (`ragdoll_core::pose`), never read back from what the layer wrote.
    pub between: Vec<u32>,
    pub above: u32,
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
/// `NiAVObject`'s local and world scale.
const LOCAL_SCALE: u32 = 0x64;
const WORLD_SCALE: u32 = 0x98;
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
/// The middle-high process level (the lowest with an animated 3D); `Actor::IsWeaponDrawn` and
/// `Actor::StopAttack`.
const MIDDLE_HIGH: u8 = 1;
const IS_WEAPON_DRAWN: u32 = 0x008a_16d0;
const STOP_ATTACK: u32 = 0x008a_8e50;

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
                    parent: None,
                    follow: Follow::REST,
                    between: Vec::new(),
                    above: 0,
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
    // Each bone's rig parent: the nearest node above it that is a rig bone.
    let nodes: Vec<u32> = found.iter().map(|b| b.node).collect();
    for bone in &mut found {
        bone.above = e.mem.u32(bone.node + PARENT);
        let mut at = bone.above;
        let mut up = 0;
        let mut between = Vec::new();
        while at != 0 && up < 32 {
            if let Some(i) = nodes.iter().position(|&n| n == at) {
                bone.parent = Some(i);
                break;
            }
            between.push(at);
            at = e.mem.u32(at + PARENT);
            up += 1;
        }
        if bone.parent.is_some() {
            between.reverse();
            bone.between = between;
        }
    }
    found
}

/// Why `actor` cannot react now (module documentation), `None` when it can.
pub fn blocked(e: &mut Engine, actor: u32) -> Option<String> {
    let process = e.mem.u32(actor + PROCESS);
    if process == 0 {
        return Some("no process".into());
    }
    let level = e.mem.u8(process + PROCESS_LEVEL);
    if level > MIDDLE_HIGH {
        return Some(format!(
            "process level {level} (below middle-high: no animated 3D)"
        ));
    }
    let knock = e.mem.u8(process + KNOCK_STATE) as i8;
    if knock != 0 {
        return Some(format!(
            "knock state {knock} (in its ragdoll or getting up)"
        ));
    }
    let life = e.mem.u32(actor + LIFE_STATE);
    if matches!(life, 1 | 2) {
        return Some(format!("life state {life} (dying or dead)"));
    }
    if crate::knockdown::in_get_up(actor) {
        return Some("getting up from a knockdown".into());
    }
    if e.call(ACTOR_3D, &args![actor]).u32() == 0 {
        return Some("no 3D".into());
    }
    None
}

fn can_flinch(e: &mut Engine, actor: u32) -> bool {
    blocked(e, actor).is_none()
}

/// Whether `actor` has its weapon drawn (`Actor::IsWeaponDrawn`, `008a16d0`).
pub fn weapon_drawn(e: &mut Engine, actor: u32) -> bool {
    e.call(IS_WEAPON_DRAWN, &args![actor]).u8() != 0
}

/// The actor's current action (its process's `+0x3e4`; 2 and 4 are attacks, as
/// `Actor::StopAttack` reads them).
pub fn current_action(e: &mut Engine, actor: u32) -> i32 {
    let process = e.mem.u32(actor + PROCESS);
    if process == 0 {
        return -1;
    }
    e.vcall(process, 0x3e4, &args![]).i32()
}

/// A big enough hit interrupts an attack (`Actor::StopAttack`, `008a8e50`, which acts only on
/// actions 2 and 4); logged with `why`.
pub fn interrupt_attack(e: &mut Engine, actor: u32, why: &str) {
    let action = current_action(e, actor);
    if !matches!(action, 2 | 4) {
        return;
    }
    e.call(STOP_ATTACK, &args![actor]);
    note(format!(
        "{}: attack interrupted ({why}; action {action})",
        describe(e, actor)
    ));
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
        armed: false,
        aim_hold: 0.0,
        body: None,
        mass: 80.0,
        hit_at: Instant::now(),
        pending: Vec::new(),
        before: None,
        planted: None,
        held: Vec::new(),
        free_wrist: [None; 2],
        hand_lock: [None; 2],
        grip_hold: false,
    });
    true
}

/// A hit below the topple speed (from `crate::knockdown`): `actor` takes the momentum `p` (N s,
/// this hit's) on its `mass` (kg) at `point` along `direction` (unit) (module documentation).
///
/// `keep_aim`: the hit is below the stagger, so an armed actor keeps its aim ([`hold_aim`]).
pub fn hit(
    e: &mut Engine,
    actor: u32,
    p: f32,
    mass: f32,
    point: V3,
    direction: V3,
    keep_aim: bool,
) {
    let strength = crate::gunplay::tuning().hit_reactions;
    if !crate::gunplay::is_on() {
        return;
    }
    if strength <= 0 {
        note(format!(
            "{}: no flinch: Hit Reactions is 0",
            describe(e, actor)
        ));
        return;
    }
    if !(p > 0.0 && mass > 0.0) {
        note(format!(
            "{}: no flinch: no momentum ({p:.2} N s on {mass:.0} kg)",
            describe(e, actor)
        ));
        return;
    }
    if let Some(why) = blocked(e, actor) {
        note(format!("{}: no flinch: {why}", describe(e, actor)));
        return;
    }
    let Some(direction) = unit(direction) else {
        note(format!(
            "{}: no flinch: the hit has no direction",
            describe(e, actor)
        ));
        return;
    };
    let v = p / mass * strength as f32 / 100.0;
    if !ensure_rig(e, actor) {
        note(format!(
            "{}: no flinch: its 3D has none of the rig's Bip01 bones",
            describe(e, actor)
        ));
        return;
    }
    let armed = weapon_drawn(e, actor);
    let action = current_action(e, actor);
    let aim_hold = if armed && keep_aim {
        ragdoll_core::tuning::get().aim_hold
    } else {
        0.0
    };
    let root = e.call(ACTOR_3D, &args![actor]).u32();
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(index) = rigs.iter().position(|r| r.actor == actor && r.root == root) else {
        return;
    };
    let rig = &mut rigs[index];
    rig.since = Instant::now();
    rig.armed = armed;
    rig.aim_hold = aim_hold;
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
    // The muscles near the hit go slack for a moment (the spread); an armed body's arms hold their
    // aim (stronger, and the torso takes the push).
    let dips: Vec<f32> = rig
        .bones
        .iter()
        .zip(&positions)
        .map(|(b, at)| {
            let d = length(sub(*at, point)) / UNITS_PER_METRE;
            let dip = MUSCLE_DIP * weight(b.role, d, spread).min(1.0);
            if b.role.is_arm() && aim_hold > 0.0 {
                dip - 5.0 * aim_hold
            } else {
                dip
            }
        })
        .collect();
    let takes: Vec<f32> = rig
        .bones
        .iter()
        .map(|b| {
            if b.role.is_arm() && aim_hold > 0.0 {
                0.0
            } else {
                1.0
            }
        })
        .collect();
    let touched = dips.iter().filter(|d| **d > 0.02).count();
    rig.mass = mass;
    rig.hit_at = Instant::now();
    rig.pending.push(Hit {
        p: p * strength as f32 / 100.0,
        dir: direction,
        at: point,
        spread,
        dips,
        takes,
    });
    let who = describe(e, actor);
    note(format!(
        "{who}: flinch at {}: {p:.1} N s on {mass:.0} kg -> {v:.2} m/s (Hit Reactions {strength}%), \
         spread {spread:.2} m, {touched} of {} bones{}; posed over its animation by nv-rs's own \
         write (every actor alike, no ragdoll-controller pass)",
        BONES
            .iter()
            .find(|(_, r)| *r == struck_role)
            .map_or("a bone", |b| b.0),
        rig.bones.len(),
        match (armed, aim_hold > 0.0, matches!(action, 2 | 4)) {
            (true, true, true) => format!("; armed and attacking (action {action}): keeps its aim"),
            (true, true, false) => "; armed: keeps its aim".to_string(),
            (true, false, _) => "; armed: the hit takes its aim".to_string(),
            (false, _, true) => format!("; attacking (action {action})"),
            _ => String::new(),
        }
    ));
}

/// Once a frame, at the draw's set-up (from `crate::gunplay`'s `FIRST_PERSON_SETUP` wrapper): every
/// reacting actor's physical body stepped toward its animation and its bones posed (module
/// documentation).
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
        if !can_flinch(e, rig.actor) {
            continue; // nothing of it is written: the animation has it
        }
        // The pose under the offset: what the animation wrote, or (while the bone still holds
        // what was written, also after the game's ragdoll-controller pass wrote it back a hair
        // off) the pose the last write was over (`ragdoll_core::pose::base_of`).
        for bone in &mut rig.bones {
            let now = (
                read9(e, bone.node + LOCAL_ROTATION),
                read3(e, bone.node + LOCAL_TRANSLATION),
            );
            bone.pose = Some(pose::base_of(now, bone.pose, bone.written));
        }
        // The animated pose in the world, worked out from the animation's own locals down from
        // the actor's root (never read back from the bones, which may hold the last write), and
        // each bone's parent node's animated world.
        let n = rig.bones.len();
        let mut world: Vec<pose::Xf> = Vec::with_capacity(n);
        let mut parents: Vec<pose::Xf> = Vec::with_capacity(n);
        for b in 0..n {
            let bone = &rig.bones[b];
            let mut up = match bone.parent {
                Some(p) => world[p],
                None => read_world(e, bone.above),
            };
            for &node in &bone.between {
                up = up.then(&read_local(e, node));
            }
            let base = bone.pose.unwrap_or_default();
            let own = pose::Xf {
                r: base.0,
                t: base.1,
                s: local_scale(e, bone.node),
            };
            parents.push(up);
            world.push(up.then(&own));
        }
        let anim: Vec<(M3, V3)> = world.iter().map(|w| (w.r, w.t)).collect();
        let Some(pi) = bone_of(&rig, Role::Pelvis, 2) else {
            continue;
        };
        // The physical body: made the frame after the first hit, from this pose and the last
        // (its motion), then the hits go in.
        if rig.body.is_none() {
            match rig.before.take() {
                Some(before) if before.len() == anim.len() && dt > 0.0 => {
                    rig.body = Some(make_body(e, &rig, &anim, &before, dt));
                }
                _ => {
                    rig.before = Some(anim.clone());
                    keep.push(rig);
                    continue;
                }
            }
        }
        let pending = std::mem::take(&mut rig.pending);
        if let Some(body) = rig.body.as_mut() {
            let m = 1.0 / UNITS_PER_METRE;
            for h in pending {
                body.hit(h.p, h.dir, scale(h.at, m), h.spread, &h.dips, &h.takes);
            }
        }
        let mut overrides = balance_frame(e, &mut rig, dt);
        // The legs keep the steps' last targets while the body goes over (no jump in them).
        if rig.balance.is_some() {
            rig.held = overrides.clone();
        } else if rig.fall.is_some() {
            for (b, o) in overrides.iter_mut().enumerate() {
                if o.is_none() && !rig.bones[b].role.is_arm() {
                    *o = rig.held.get(b).copied().flatten();
                }
            }
        }
        brace_frame(e, &mut rig, &anim, &mut overrides, dt);
        // The grip hold (`home_of`): once the support hand is back on a two-handed weapon's grip
        // after a brace, its arm's motors stay on the grip while the body is physical (the
        // physical arm never swings off the weapon as the brace hands it back).
        if rig.body.is_none() || !grips(&rig, &anim) {
            rig.grip_hold = false;
        } else if rig
            .brace
            .and_then(|b| b.hands[0])
            .is_some_and(|h| h.on_home())
        {
            rig.grip_hold = true;
        }
        let support_bracing = rig
            .brace
            .and_then(|b| b.hands[0])
            .is_some_and(|h| !h.released());
        if rig.grip_hold && !support_bracing {
            if let (Some(grip), Some(u), Some(f), Some(h)) = (
                home_of(&rig, &anim, 0, |i| world_of(e, &rig, i)),
                bone_of(&rig, Role::UpperArm, 0),
                bone_of(&rig, Role::Forearm, 0),
                bone_of(&rig, Role::Hand, 0),
            ) {
                let shoulder = read3(e, rig.bones[u].node + WORLD_TRANSLATION);
                let goal = add(anim[u].1, sub(grip.1, shoulder));
                let fwd = facing(e, &rig);
                if let Some((r1, r2)) = reach_arm_ik(anim[u].1, anim[f].1, anim[h].1, goal, fwd, 0)
                {
                    overrides[u] = Some(r1);
                    overrides[f] = Some(r2);
                }
            }
        }
        if fall_frame(e, &mut rig, &mut overrides, dt) {
            continue; // in the ragdoll now: it owns the bones
        }
        // The targets: the animation, with the steps' legs, the brace's arms, the windmill (each
        // override a world turn carried down its chain).
        let n = rig.bones.len();
        let mut chain = vec![IDENTITY; n];
        let mut target = vec![IDENTITY; n];
        for b in 0..n {
            let up = rig.bones[b].parent.map_or(IDENTITY, |p| chain[p]);
            chain[b] = match overrides[b] {
                Some(o) => mul(&up, &o),
                None => up,
            };
            target[b] = mul(&chain[b], &anim[b].0);
        }
        let pelvis_target = add(anim[pi].1, rig.shift);
        let busy = rig.balance.is_some() || rig.brace.is_some() || rig.fall.is_some();
        if busy {
            rig.since = Instant::now();
        }
        // The feet: where the steps put them; after the steps, eased back to the animation's
        // over [`FEET_BACK`] (no jump; an actor its AI walks on keeps its feet under it).
        let feet_now = match (
            &rig.balance,
            bone_of(&rig, Role::Foot, 0),
            bone_of(&rig, Role::Foot, 1),
        ) {
            (_, Some(l), Some(r)) => {
                let animated = [anim[l].1, anim[r].1];
                let scale_ = actor_scale(e, rig.actor);
                let at = match &rig.balance {
                    Some(s) => {
                        let f = s.feet(scale_);
                        rig.planted = Some((f, Instant::now()));
                        f
                    }
                    None => match rig.planted {
                        Some((p, since)) => {
                            let k = active::ease(since.elapsed().as_secs_f32() / FEET_BACK);
                            [0, 1].map(|i| add(p[i], scale(sub(animated[i], p[i]), k)))
                        }
                        None => animated,
                    },
                };
                vec![(l, at[0]), (r, at[1])]
            }
            _ => Vec::new(),
        };
        let support = match &rig.fall {
            Some(f) => 1.0 - active::ease(f.t / f.lead.max(1e-3)),
            None => 1.0,
        };
        let since = rig.since.elapsed().as_secs_f32();
        let world: Vec<f32> = rig
            .bones
            .iter()
            .map(|b| {
                if b.role.is_arm() && b.side < 2 {
                    // The aim kept, or the brace's reach to the wall point (not riding up with a
                    // torso pitched toward the wall); an armed actor's weapon arm on its aim while
                    // its free arm braces.
                    let side = b.side as usize;
                    let (brace, aims) = rig.brace.map_or((0.0, false), |br| {
                        (br.weight(side), br.aims(side, rig.armed))
                    });
                    if aims {
                        1.0
                    } else {
                        (rig.aim_hold * 1.25)
                            .max(brace * ragdoll_core::tuning::get().brace_world)
                            .min(1.0)
                    }
                } else {
                    0.0
                }
            })
            .collect();
        let m = 1.0 / UNITS_PER_METRE;
        let mut shown: Vec<M3> = target.clone();
        let mut pelvis_offset = rig.shift;
        let mut failed: Option<String> = None;
        let forward = facing(e, &rig);
        let cond = condition_of(rig.actor);
        let calves: [Option<usize>; 2] =
            [bone_of(&rig, Role::Calf, 0), bone_of(&rig, Role::Calf, 1)];
        // A planted brace hand's wrist held where it touched, as a contact
        // (`ragdoll_core::brace::Hand::pin`).
        let pins: Vec<(usize, V3, f32, V3)> = rig
            .brace
            .iter()
            .flat_map(|br| br.hands.iter().flatten())
            .filter_map(|h| {
                let (at, share, normal) = h.pin()?;
                Some((
                    bone_of(&rig, Role::Hand, h.side)?,
                    scale(at, m),
                    share,
                    normal,
                ))
            })
            .collect();
        let armed = rig.armed;
        let brace_now = rig.brace;
        let mut phys_w = 0.0f32;
        if let Some(body) = rig.body.as_mut() {
            let mut targets = active::Targets {
                rot: target.iter().map(active::qfrom_m).collect(),
                pelvis: scale(pelvis_target, m),
                feet: feet_now.iter().map(|&(i, at)| (i, scale(at, m))).collect(),
                hands: pins.iter().map(|p| (p.0, p.1)).collect(),
                hand_share: pins.iter().map(|p| p.2).collect(),
                hand_normal: pins.iter().map(|p| p.3).collect(),
                support,
                world,
                anim: anim.iter().map(|a| active::qfrom_m(&a.0)).collect(),
                forward,
            };
            // A deliberate brace: the bracing arms' motors as firm as the tuning says while they
            // reach and hold; an armed actor's weapon arm held firm on its aim.
            if let Some(br) = brace_now {
                for (b, d) in body.dip.iter_mut().enumerate() {
                    let bone = &rig.bones[b];
                    if bone.role.is_arm() && bone.side < 2 {
                        let side = bone.side as usize;
                        if br.aims(side, armed) {
                            *d = d.min(-AIM_FIRM);
                        } else {
                            *d = d.min(-ragdoll_core::tuning::get().brace_firm * br.weight(side));
                        }
                    }
                }
            }
            // What earlier hits left (`ragdoll_core::condition`): a hurt arm's motors weak (it
            // dangles), a hurt leg's knee bent more and the pelvis lower over it, fatigue weakening
            // every motor.
            for (b, d) in body.dip.iter_mut().enumerate() {
                let bone = &rig.bones[b];
                if bone.role.is_arm() && bone.side < 2 && rig.brace.is_none() {
                    let dangle = cond.dangle(bone.side as usize);
                    if dangle > 0.0 {
                        *d = d.max(dangle);
                    }
                }
            }
            for (side, calf) in calves.iter().enumerate() {
                let (knee, drop) = cond.sag(side);
                if let (Some(c), true) = (calf, knee > 0.0) {
                    body.flex(&mut targets.rot, *c, knee);
                    targets.pelvis[2] -= drop * 0.5;
                }
            }
            body.step(
                &targets,
                dt,
                active::strength(rig.hit_at.elapsed().as_secs_f32()) * cond.motor_scale(),
            );
            if let Err(why) = body.check() {
                failed = Some(format!("the body: {why}"));
            }
            let w = active::weight(since);
            phys_w = w;
            let pose = body.pose();
            // The targets as the body tracks them (no elbow or knee past its human range) and the
            // body, blended (`Body::shown`: the elbows and knees against their parents).
            let wanted: Vec<active::Q> = target.iter().map(active::qfrom_m).collect();
            for (b, q) in body.shown(&wanted, w).into_iter().enumerate() {
                shown[b] = active::m_from_q(q);
            }
            let sim = scale(pose[pi].1, UNITS_PER_METRE);
            pelvis_offset = add(rig.shift, scale(sub(sim, pelvis_target), w));
            if w <= 0.0 && !busy {
                rig.body = None;
            }
        }
        // A brace hand on its path, plant or way home: the shown arm put on it
        // (`brace::plant_arm`), solved from the arm as it was shown a frame before (the bones' last
        // write) on the shoulder where it is now, so it moves on from itself whatever the physical
        // arm under it does, blended with the free arm by the plant's weight; the free arms' wrists
        // kept (the hand-over waits for them). Then the grip hold.
        if rig.brace.is_some() || rig.grip_hold {
            let shown_at = shown_positions(&rig, &anim, &shown, pi, pelvis_offset);
            for side in 0..2u8 {
                rig.free_wrist[side as usize] =
                    bone_of(&rig, Role::Hand, side).map(|h| shown_at[h]);
            }
            // A bone as the shown pose has it so far (rotation and joint).
            let now = |shown: &[M3], at: &[V3], i: usize| Some((shown[i], at[i]));
            let mut put: Vec<PutArm> = Vec::new();
            if let Some(br) = rig.brace {
                for hand in br.hands.iter().flatten() {
                    let (Some(goal), w) = (hand.goal(), hand.plant_weight()) else {
                        continue;
                    };
                    let home = home_of(&rig, &anim, hand.side, |i| now(&shown, &shown_at, i));
                    // Let go: on its way back home, the home as this frame has it (no lag behind
                    // the weapon hand that carries a grip); the hand turned onto it as it comes.
                    let (goal, turn) = match (hand.home_offset(), home) {
                        (Some(off), Some(home)) => (
                            add(home.1, off),
                            Some((home.0, active::ease(hand.home_share()))),
                        ),
                        _ => (goal, None),
                    };
                    put.push(PutArm {
                        side: hand.side,
                        goal,
                        w,
                        turn,
                        planted: hand.planted(),
                    });
                }
            }
            let support_bracing = rig
                .brace
                .and_then(|b| b.hands[0])
                .is_some_and(|h| !h.released());
            if rig.grip_hold && !support_bracing && grips(&rig, &anim) {
                if let Some(grip) = home_of(&rig, &anim, 0, |i| now(&shown, &shown_at, i)) {
                    put.retain(|p| p.side != 0);
                    put.push(PutArm {
                        side: 0,
                        goal: grip.1,
                        w: phys_w.clamp(0.0, 1.0),
                        turn: Some((grip.0, 1.0)),
                        planted: false,
                    });
                }
            }
            for PutArm {
                side,
                goal,
                w,
                turn,
                planted,
            } in put
            {
                let (Some(u), Some(f), Some(h)) = (
                    bone_of(&rig, Role::UpperArm, side),
                    bone_of(&rig, Role::Forearm, side),
                    bone_of(&rig, Role::Hand, side),
                ) else {
                    continue;
                };
                let before = |i: usize| {
                    (
                        read9(e, rig.bones[i].node + WORLD_ROTATION),
                        read3(e, rig.bones[i].node + WORLD_TRANSLATION),
                    )
                };
                let (pu, pf, ph) = (before(u), before(f), before(h));
                let shoulder = shown_at[u];
                let elbow = add(shoulder, sub(pf.1, pu.1));
                let wrist = add(elbow, sub(ph.1, pf.1));
                let Some((r1, r2)) = hands::plant_arm(
                    shoulder,
                    elbow,
                    wrist,
                    goal,
                    1.0,
                    forward,
                    hands::MOST_TWIST * dt,
                ) else {
                    continue;
                };
                let iu = mul(&r1, &pu.0);
                let ifo = mul(&mul(&r1, &r2), &pf.0);
                // The hand with the arm: held as it touched once planted (the palm stays on the
                // surface); on its way home turned onto it (from as it let go); else carried by
                // its forearm.
                let carried = mul(&ifo, &mul(&transpose(&pf.0), &ph.0));
                let ih = if planted {
                    *rig.hand_lock[side as usize].get_or_insert(ph.0)
                } else if let Some((home, k)) = turn {
                    let from = rig.hand_lock[side as usize].unwrap_or(carried);
                    active::m_from_q(active::qslerp(
                        active::qfrom_m(&from),
                        active::qfrom_m(&home),
                        k,
                    ))
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
                shown[u] = pose::ortho(&blend(shown[u], iu));
                shown[f] = pose::ortho(&blend(shown[f], ifo));
                shown[h] = pose::ortho(&blend(shown[h], ih));
            }
        }
        // A two-handed weapon's support hand off its grip (bracing, coming back): its shots go
        // wider by how far (`crate::one_hand`).
        if grips(&rig, &anim) {
            let off = if rig.brace.is_some() || rig.grip_hold {
                let at = shown_positions(&rig, &anim, &shown, pi, pelvis_offset);
                let grip = home_of(&rig, &anim, 0, |i| Some((shown[i], at[i])));
                match (bone_of(&rig, Role::Hand, 0), grip) {
                    (Some(h), Some(g)) => ragdoll_core::one_hand::off_grip(length(sub(at[h], g.1))),
                    _ => 0.0,
                }
            } else {
                0.0
            };
            crate::one_hand::set(rig.actor, off);
        }
        // Each bone's world offset over its animation: its parent's shown and animated rotations
        // taken back, its own shown one on (`A(p) S(p)^T S(b) A(b)^T`), made orthonormal.
        let offsets: Vec<M3> = (0..n)
            .map(|b| {
                let own = mul(&shown[b], &transpose(&anim[b].0));
                pose::ortho(&match rig.bones[b].parent {
                    Some(p) => mul(&mul(&anim[p].0, &transpose(&shown[p])), &own),
                    None => own,
                })
            })
            .collect();
        // Anything not believable (a body gone wrong, an offset that is no rotation, a pelvis
        // pushed far, an animated pose not where the actor is): the body goes and the animation
        // has the bones again, said once.
        if failed.is_none() {
            let joints: Vec<V3> = anim.iter().map(|a| a.1).collect();
            let at = read3(e, root + WORLD_TRANSLATION);
            if let Err(why) = pose::sane(&offsets, pelvis_offset, &joints, at) {
                failed = Some(why);
            }
        }
        if let Some(why) = failed {
            note(format!(
                "{}: the active ragdoll is dropped back to the animation: {why}",
                describe(e, rig.actor)
            ));
            rig.body = None;
            rig.balance = None;
            rig.brace = None;
            rig.fall = None;
            rig.shift = [0.0; 3];
            rig.pending.clear();
            // Its bones put back to the animation's pose, then it goes.
            for bone in &mut rig.bones {
                if let Some((rot, t)) = bone.pose {
                    write9(e, bone.node + LOCAL_ROTATION, &rot);
                    write3(e, bone.node + LOCAL_TRANSLATION, t);
                }
            }
            update(e, root);
            continue;
        }
        let resting = rig.body.is_none() && !busy;
        // What each bone is written with: the local its shown pose needs (absolute, the same
        // however often it is written; `ragdoll_core::pose::local_for`).
        let locals: Vec<(M3, V3)> = (0..n)
            .map(|b| {
                let bone = &rig.bones[b];
                let base = bone.pose.unwrap_or_default();
                if resting {
                    base
                } else {
                    let shift = (bone.role == Role::Pelvis).then_some(pelvis_offset);
                    pose::local_for(&parents[b], base, &offsets[b], shift)
                }
            })
            .collect();
        for (bone, &(rot, t)) in rig.bones.iter_mut().zip(&locals) {
            if rot.iter().all(|x| x.is_finite()) && t.iter().all(|x| x.is_finite()) {
                write9(e, bone.node + LOCAL_ROTATION, &rot);
                write3(e, bone.node + LOCAL_TRANSLATION, t);
                bone.written = Some((rot, t));
            }
        }
        update(e, root);
        if resting && rig.since.elapsed().as_secs_f32() > 0.2 {
            note(format!("{}: back in its animation", describe(e, rig.actor)));
            continue;
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

/// The shown pose's joints (world, game units): each bone's animated offset from its parent turned
/// by the parent's shown rotation over its animated one, down from the pelvis moved by
/// `pelvis_offset`.
fn shown_positions(
    rig: &Rig,
    anim: &[(M3, V3)],
    shown: &[M3],
    pelvis: usize,
    pelvis_offset: V3,
) -> Vec<V3> {
    let n = rig.bones.len();
    let mut at: Vec<V3> = Vec::with_capacity(n);
    for b in 0..n {
        let p = match rig.bones[b].parent {
            Some(p) => add(
                at[p],
                apply_m(
                    &mul(&shown[p], &transpose(&anim[p].0)),
                    sub(anim[b].1, anim[p].1),
                ),
            ),
            None if b == pelvis => add(anim[b].1, pelvis_offset),
            None => anim[b].1,
        };
        at.push(p);
    }
    at
}

/// An arm put on a goal in the shown pose (a brace hand, the grip hold): its side, the wrist's
/// goal, the weight, the hand's home rotation and how far it is turned onto it, and whether the
/// hand is planted (its rotation held as it touched).
struct PutArm {
    side: u8,
    goal: V3,
    w: f32,
    turn: Option<(M3, f32)>,
    planted: bool,
}

/// Where a released brace hand of `side` goes back to (rotation and wrist, world): the support
/// (left) hand of an actor holding its weapon two-handed (`ragdoll_core::brace::grips`: armed, its
/// animation's hands close) to its grip, as the weapon (right) hand carries it; else the
/// animation's hand as the bone its arm hangs from carries it. `now` gives a bone as it is now.
fn home_of(
    rig: &Rig,
    anim: &[(M3, V3)],
    side: u8,
    now: impl Fn(usize) -> Option<(M3, V3)>,
) -> Option<(M3, V3)> {
    let h = bone_of(rig, Role::Hand, side)?;
    if side == 0 && grips(rig, anim) {
        let w = bone_of(rig, Role::Hand, 1)?;
        return Some(hands::carried(anim[w], anim[h], now(w)?));
    }
    let u = bone_of(rig, Role::UpperArm, side)?;
    let c = rig.bones[u].parent?;
    Some(hands::carried(anim[c], anim[h], now(c)?))
}

/// Whether the actor holds its weapon two-handed now (`ragdoll_core::brace::grips`).
fn grips(rig: &Rig, anim: &[(M3, V3)]) -> bool {
    match (bone_of(rig, Role::Hand, 0), bone_of(rig, Role::Hand, 1)) {
        (Some(l), Some(r)) => hands::grips(rig.armed, anim[l].1, anim[r].1),
        _ => false,
    }
}

/// A rig bone as it was last written (world rotation and position).
fn world_of(e: &Engine, rig: &Rig, i: usize) -> Option<(M3, V3)> {
    let node = rig.bones.get(i)?.node;
    Some((
        read9(e, node + WORLD_ROTATION),
        read3(e, node + WORLD_TRANSLATION),
    ))
}

/// An armed actor's weapon arm held on its aim while its free arm braces: its motors this many
/// times firmer (the active body's dip below 0).
const AIM_FIRM: f32 = 4.0;

/// The feet go back from where the steps left them to the animation's over this long (s), eased.
const FEET_BACK: f32 = 0.4;

/// A leaf bone's body reaches this far (m) along its parent's direction (the rig has no bone
/// past it), and each role's radius (m), at scale 1.
fn leaf_length(role: Role) -> f32 {
    match role {
        Role::Head => 0.22,
        Role::Hand => 0.12,
        Role::Foot => 0.12,
        _ => 0.1,
    }
}
fn radius(role: Role) -> f32 {
    match role {
        Role::Pelvis => 0.12,
        Role::Spine => 0.13,
        Role::Neck => 0.05,
        Role::Head => 0.1,
        Role::Clavicle => 0.05,
        Role::UpperArm => 0.06,
        Role::Forearm => 0.05,
        Role::Hand => 0.045,
        Role::Thigh => 0.085,
        Role::Calf => 0.065,
        Role::Foot => 0.045,
    }
}

/// The rig's physical body (`ragdoll_core::active`) in the pose `now`, moving as it moved from
/// `before` over `dt`: each bone a capsule to the next rig bone down its chain (the middle one: the
/// spine's, the neck's), its mass its share of the body's.
fn make_body(
    e: &mut Engine,
    rig: &Rig,
    now: &[(M3, V3)],
    before: &[(M3, V3)],
    dt: f32,
) -> active::Body {
    let m = 1.0 / UNITS_PER_METRE;
    let scale_ = actor_scale(e, rig.actor);
    let n = rig.bones.len();
    let shares: Vec<f32> = rig
        .bones
        .iter()
        .map(|b| {
            let name = e.mem.u32(b.node + NODE_NAME);
            if name == 0 {
                0.01
            } else {
                let text = String::from_utf8_lossy(&e.mem.cstr(name)).to_string();
                active::share(&text)
            }
        })
        .collect();
    let total: f32 = shares.iter().sum::<f32>().max(1e-3);
    let bones: Vec<active::Bone> = (0..n)
        .map(|b| {
            let bone = &rig.bones[b];
            let children: Vec<usize> = (0..n).filter(|&c| rig.bones[c].parent == Some(b)).collect();
            let next = children
                .iter()
                .copied()
                .find(|&c| matches!(rig.bones[c].role, Role::Spine | Role::Neck | Role::Head))
                .or_else(|| children.first().copied());
            let end = match next {
                Some(c) => now[c].1,
                None => {
                    let from = bone
                        .parent
                        .map_or(add(now[b].1, [0.0, 0.0, -1.0]), |p| now[p].1);
                    let dir = unit(sub(now[b].1, from)).unwrap_or([0.0, 0.0, 1.0]);
                    add(
                        now[b].1,
                        scale(dir, leaf_length(bone.role) * scale_ * UNITS_PER_METRE),
                    )
                }
            };
            active::Bone {
                parent: bone.parent,
                role: bone.role,
                q: active::qfrom_m(&now[b].0),
                joint: scale(now[b].1, m),
                end: scale(end, m),
                radius: radius(bone.role) * scale_,
                mass: rig.mass * shares[b] / total,
                before: Some((active::qfrom_m(&before[b].0), scale(before[b].1, m))),
            }
        })
        .collect();
    active::Body::from_pose(&bones, dt)
}

/// The physical body's bones' velocities (game units a second) by node, for the ragdoll to go on
/// with when it takes the body (`crate::knockdown`): nothing jumps at the switch.
pub fn body_velocities(actor: u32) -> Vec<(u32, V3)> {
    let rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(rig) = rigs.iter().find(|r| r.actor == actor) else {
        return Vec::new();
    };
    let Some(body) = &rig.body else {
        return Vec::new();
    };
    rig.bones
        .iter()
        .zip(&body.state)
        .map(|(b, s)| (b.node, scale(s.v, UNITS_PER_METRE)))
        .collect()
}

// --- The write: one path for every actor, over its animation (module documentation). ----------

/// A node's local transform (rotation, translation, scale).
fn read_local(e: &Engine, node: u32) -> pose::Xf {
    pose::Xf {
        r: pose::ortho(&read9(e, node + LOCAL_ROTATION)),
        t: read3(e, node + LOCAL_TRANSLATION),
        s: local_scale(e, node),
    }
}

/// A node's world transform (none: the identity).
fn read_world(e: &Engine, node: u32) -> pose::Xf {
    if node == 0 {
        return pose::Xf::IDENTITY;
    }
    let s = e.mem.f32(node + WORLD_SCALE);
    pose::Xf {
        r: pose::ortho(&read9(e, node + WORLD_ROTATION)),
        t: read3(e, node + WORLD_TRANSLATION),
        s: if s.is_finite() && s > 0.0 { s } else { 1.0 },
    }
}

fn local_scale(e: &Engine, node: u32) -> f32 {
    let s = e.mem.f32(node + LOCAL_SCALE);
    if s.is_finite() && s > 0.0 {
        s
    } else {
        1.0
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
    use crate::balance::Outcome;
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
            fold: None,
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
        let (hip, ankle) = (add(hip0, shift), add(ankle0, shift));
        let target = add(ankle, scale(sub(feet[side as usize], ankle), s.weight));
        // The knee forward whichever way the push goes (`leg_ik`: never bent backwards).
        let Some((r1, m, level)) = leg_ik(hip0, knee0, ankle0, hip, target, facing(e, rig)) else {
            continue;
        };
        out[t] = Some(r1);
        out[c] = Some(m);
        out[f] = Some(level);
    }
    // No scripted arms: the physical body's arms swing out of their own as it steps.
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
            fold: None,
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
    use crate::balance::{plan, Outcome, Stepping};
    let strength = crate::gunplay::tuning().balance;
    if strength <= 0 || !crate::gunplay::is_on() || !(p > 0.0 && mass > 0.0) {
        return; // Balance off (or nothing pushed): the flinch alone, said by `hit`
    }
    if let Some(why) = blocked(e, actor) {
        note(format!("{}: no catch steps: {why}", describe(e, actor)));
        return;
    }
    if !ensure_rig(e, actor) {
        note(format!(
            "{}: no catch steps: its 3D has none of the rig's Bip01 bones",
            describe(e, actor)
        ));
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
    let (feet, carried) = match &rig.balance {
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
    // Pushed past its feet: no aim kept, and an attack is interrupted (below, without the lock).
    rig.aim_hold = 0.0;
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
            fold: None,
        });
        rig.balance = None;
        drop(rigs);
        note(format!(
            "{who}: on one leg it cannot step: falls ({v:.2} m/s)"
        ));
        interrupt_attack(e, actor, "it falls");
        return;
    }
    let outcome =
        if planned.need > crate::balance::STEP_LENGTH * scale_ * crate::balance::MAX_STEPS as f32 {
            Outcome::Overrun
        } else {
            Outcome::Caught
        };
    let start = feet;
    drop(rigs);
    interrupt_attack(e, actor, "pushed past its feet");
    // Each foot's ankle over its ground (the navmesh's height is the ground's; the foot bone is
    // the ankle): kept for its steps.
    let ankle: [f32; 2] = [0, 1].map(|f| {
        ground_point(e, actor, start[f])
            .map(|g| start[f][2] - g[2])
            .filter(|h| (0.0..=25.0).contains(h))
            .unwrap_or(8.0 * scale_)
    });
    // The steps on the world (`ragdoll_core::obstacle::resolve`, the lab's too): floors, walls,
    // ledges, a waist-high obstacle or a wall the body meets along the push (a railing's gaps let
    // a foot's knee-height ray through, not the body), and low obstacles a swinging foot catches.
    let floor_z = ground_point(e, actor, pelvis).map_or(start[0][2] - ankle[0], |g| g[2]);
    let ahead = obstacle::scan(
        &mut GameWorld { e, actor },
        pelvis,
        floor_z,
        dir,
        planned.need + BRACE_PROBE,
        scale_,
    )
    .map(|o| (pelvis, dir, o));
    let r = obstacle::resolve(
        &mut GameWorld { e, actor },
        &planned.steps,
        feet,
        ankle,
        scale_,
        outcome,
        ahead,
    );
    let (mut steps, mut outcome, mut said) = (r.steps, r.outcome, r.said);
    if let (Outcome::Trip, Some(o)) = (outcome, r.tripped) {
        if obstacle::trips_over(v, condition_of(actor).leg_weakness()) {
            begin_fall(
                e,
                actor,
                root,
                Fall {
                    dir,
                    speed: v,
                    aggressor,
                    point: pelvis,
                    t: 0.0,
                    lead: FALL_LEAD,
                    windmill: false,
                    fold: None,
                },
            );
            note(format!(
                "{who}: pushed {v:.2} m/s: its foot caught on a {:.0}-unit {}, it pitches over it",
                o.height,
                o.kind.name()
            ));
            room_event(
                e,
                actor,
                "trip",
                &format!(
                    "\"speed\":{},\"height\":{},\"on\":{},\"falls\":true",
                    room_events::num(v),
                    room_events::num(o.height),
                    room_events::text(o.kind.name())
                ),
            );
            return;
        }
        // Little push left: it stumbles, then steps over it.
        if let Some(&(foot, caught)) = steps.last() {
            if let Some(to) = obstacle::recovery_step(
                &mut GameWorld { e, actor },
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
        room_event(
            e,
            actor,
            "trip",
            &format!(
                "\"speed\":{},\"height\":{},\"on\":{},\"falls\":false",
                room_events::num(v),
                room_events::num(o.height),
                room_events::text(o.kind.name())
            ),
        );
    }
    let against_wall = outcome == Outcome::Wall;
    if against_wall {
        start_brace(e, actor, root, dir, v, aggressor);
    }
    room_event(
        e,
        actor,
        "steps",
        &format!(
            "\"speed\":{},\"need\":{},\"count\":{},\"outcome\":{},\"said\":[{}]",
            room_events::num(v),
            room_events::num(planned.need / UNITS_PER_METRE),
            steps.len(),
            room_events::text(&format!("{outcome:?}")),
            said.iter()
                .map(|s| room_events::text(s))
                .collect::<Vec<_>>()
                .join(",")
        ),
    );
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
        // No arm to brace with (`start_brace` made none): nothing reaches.
        rig.brace = Some(Brace {
            dir,
            t: 0.0,
            wall: None,
            top: None,
            surface: None,
            hands: [None, None],
        });
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

// --- The world, by the game's own ray casts (`jip::commands::world_ray_cast`). ------------------

/// How far down a floor is looked for under a point (units), how far up from it the cast starts;
/// how far ahead a wall or an obstacle is looked for (`obstacle::scan`, the shoulders' ray).
const FLOOR_FROM: f32 = 50.0;
const FLOOR_RANGE: f32 = 300.0;
const BRACE_PROBE: f32 = 90.0;

/// The game's ray cast (`jip::commands::world_ray_cast`) from `from` along `dir` (unit) within
/// `range` (game units), the world only: a hit on an actor (the reacting actor's own limbs, which
/// JIP's default layer meets, or anyone else's) is passed and the cast goes on beyond it, at most
/// a few times. The hit point.
fn cast_world(e: &mut Engine, from: V3, dir: V3, range: f32) -> Option<V3> {
    let mut at = from;
    let mut left = range;
    for _ in 0..4 {
        let (hit, refr) = jip::commands::world_ray_cast(e, at, dir, left)?;
        if !hit.iter().all(|x| x.is_finite()) {
            return None;
        }
        // The reference hit (never the scene block itself: taken for a reference, its vtable was
        // read past its end and the game crashed, 022ac96), its `IsActor` slot compared with the
        // game's answer-true stub, not called (`jip::commands::is_actor`).
        if !jip::commands::is_actor(e, refr) {
            return Some(hit);
        }
        let gone = length(sub(hit, at)) + 2.0;
        left -= gone;
        if left <= 0.0 {
            return None;
        }
        at = add(at, scale(dir, gone));
    }
    None
}

/// The floor under `at` (game units): the game's ray cast straight down from a little above it,
/// else the navmesh's nearest point (`crate::knockdown::navmesh_point`).
fn ground_point(e: &mut Engine, actor: u32, at: V3) -> Option<V3> {
    let from = [at[0], at[1], at[2] + FLOOR_FROM];
    if let Some(hit) = cast_world(e, from, [0.0, 0.0, -1.0], FLOOR_RANGE) {
        if hit[2] <= at[2] + FLOOR_FROM - 5.0 {
            return Some(hit);
        }
    }
    crate::knockdown::navmesh_point(e, actor, at)
}

/// How far ahead of `at` along `dir` (flat) a wall is (game units), within `range`.
fn wall_ahead(e: &mut Engine, at: V3, dir: V3, range: f32) -> Option<f32> {
    let dir = unit([dir[0], dir[1], 0.0])?;
    let hit = cast_world(e, at, dir, range)?;
    let d = length(sub([hit[0], hit[1], 0.0], [at[0], at[1], 0.0]));
    d.is_finite().then_some(d)
}

/// The game's world as a reacting body asks it (`ragdoll_core::obstacle::World`): the floor
/// ([`ground_point`]) and the game's ray cast past actors ([`cast_world`]).
struct GameWorld<'a> {
    e: &'a mut Engine,
    actor: u32,
}

impl obstacle::World for GameWorld<'_> {
    fn floor(&mut self, at: V3) -> Option<V3> {
        ground_point(self.e, self.actor, at)
    }
    fn cast(&mut self, from: V3, dir: V3, range: f32) -> Option<f32> {
        cast_world(self.e, from, dir, range).map(|hit| length(sub(hit, from)))
    }
}

/// What a brace meets along `dir` (the lab's `brace_obstacle`): the obstacle's distance from the
/// pelvis, its top when it is waist-high (the hands go on it), and its kind; the game's ray casts
/// at several heights and across the body (`obstacle::scan`), else the shoulders' ray.
fn brace_obstacle(
    e: &mut Engine,
    actor: u32,
    root: u32,
    dir: V3,
) -> (Option<f32>, Option<f32>, Option<obstacle::Kind>, Option<V3>) {
    let (pelvis, shoulders) = {
        let rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
        let Some(rig) = rigs.iter().find(|r| r.actor == actor && r.root == root) else {
            return (None, None, None, None);
        };
        let at =
            |role| bone_of(rig, role, 2).map(|i| read3(e, rig.bones[i].node + WORLD_TRANSLATION));
        (at(Role::Pelvis), at(Role::UpperArm))
    };
    let scale_ = actor_scale(e, actor);
    if let Some(at) = pelvis {
        let floor_z = ground_point(e, actor, at).map_or(at[2] - 60.0, |g| g[2]);
        if let Some(o) = obstacle::scan(
            &mut GameWorld { e, actor },
            at,
            floor_z,
            dir,
            BRACE_PROBE,
            scale_,
        ) {
            let top = (o.kind == obstacle::Kind::Waist).then_some(o.top);
            return (Some(o.distance), top, Some(o.kind), Some(at));
        }
    }
    (
        shoulders.and_then(|at| wall_ahead(e, at, dir, BRACE_PROBE)),
        None,
        None,
        shoulders,
    )
}

/// A fall begun from the steps (a stumble that pitches over): the rig's fall set, its steps ended.
fn begin_fall(e: &mut Engine, actor: u32, root: u32, fall: Fall) {
    let _ = e;
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(rig) = rigs.iter_mut().find(|r| r.actor == actor && r.root == root) {
        if rig.fall.is_none() {
            rig.fall = Some(fall);
        }
        rig.balance = None;
    }
}

// --- What hits leave in an actor (`ragdoll_core::condition`), kept per actor across reactions. --

/// Each actor's condition and when it was last brought up to date.
static CONDITIONS: Mutex<Vec<(u32, Condition, Instant)>> = Mutex::new(Vec::new());

/// How many actors' conditions are kept (the most recently hit).
const CONDITIONS_KEPT: usize = 64;

/// The actor values of the limbs' condition: left attack (arm), right attack, left mobility
/// (leg), right mobility (FNV's `LeftAttackCondition` 27 to `RightMobilityCondition` 30).
const LIMB_CONDITIONS: [(u32, condition::Region); 4] = [
    (27, condition::Region::LeftArm),
    (28, condition::Region::RightArm),
    (29, condition::Region::LeftLeg),
    (30, condition::Region::RightLeg),
];

/// `actor`'s condition now (rested since it was last hit).
pub fn condition_of(actor: u32) -> Condition {
    let mut all = CONDITIONS.lock().unwrap_or_else(|p| p.into_inner());
    match all.iter_mut().find(|c| c.0 == actor) {
        Some(c) => {
            c.1.decay(c.2.elapsed().as_secs_f32());
            c.2 = Instant::now();
            c.1
        }
        None => Condition::default(),
    }
}

/// A hit of `severity` (its own push over the topple speed) at `point` on `actor`: its condition
/// updated (fatigue, pain, the struck limb's injury; a crippled limb fully hurt) and the stage
/// pain alone escalates this hit to (`crate::knockdown::after_hit`).
pub fn condition_hit(e: &mut Engine, actor: u32, severity: f32, point: V3) -> condition::Stage {
    let region = {
        let rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
        rigs.iter()
            .find(|r| r.actor == actor)
            .and_then(|rig| {
                rig.bones
                    .iter()
                    .min_by(|a, b| {
                        let da = length(sub(read3(e, a.node + WORLD_TRANSLATION), point));
                        let db = length(sub(read3(e, b.node + WORLD_TRANSLATION), point));
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|b| condition::region_of(b.role, b.side))
            })
            .unwrap_or(condition::Region::Torso)
    };
    let mut c = condition_of(actor);
    c.hit(severity, region);
    for (av, limb) in LIMB_CONDITIONS {
        let v = e.vcall(actor + 0xa4, 0xc, &args![av]).f64() as f32;
        if v.is_finite() && v <= 0.0 {
            c.cripple(limb);
        }
    }
    let mut all = CONDITIONS.lock().unwrap_or_else(|p| p.into_inner());
    all.retain(|x| x.0 != actor);
    all.push((actor, c, Instant::now()));
    if all.len() > CONDITIONS_KEPT {
        all.remove(0);
    }
    c.stage()
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
    drop(rigs);
    // What the brace meets (`brace_obstacle`): a waist-high obstacle's top the hands go on.
    let (wall, top, kind, origin) = brace_obstacle(e, actor, root, dir);
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(rig) = rigs.iter_mut().find(|r| r.actor == actor && r.root == root) else {
        return;
    };
    let mut which = [false; 2];
    if arms {
        let surface = origin.and_then(|o| Surface::new(o, dir, wall, top));
        // Which arms brace (`ragdoll_core::brace::bracing_arms`): an armed actor's free arm, its
        // weapon arm too only on a push past what one arm holds or when the free arm cannot
        // reach the surface (the weapon kept in hand).
        let mut have = [false; 2];
        let mut reaches = [false; 2];
        for side in 0..2u8 {
            have[side as usize] = rig.limbs.arm(side as usize);
            if let (Some(u), Some(f), Some(h)) = (
                bone_of(rig, Role::UpperArm, side),
                bone_of(rig, Role::Forearm, side),
                bone_of(rig, Role::Hand, side),
            ) {
                let at = |i: usize| read3(e, rig.bones[i].node + WORLD_TRANSLATION);
                let reach = length(sub(at(f), at(u))) + length(sub(at(h), at(f)));
                let target = brace_target(at(u), dir, wall, top);
                reaches[side as usize] =
                    surface.is_none() || hands::can_reach(at(u), target, reach);
            }
        }
        let one_arm = limit * hands::ONE_ARM;
        which = hands::bracing_arms(have, rig.armed, v, one_arm, reaches);
        rig.hand_lock = [None; 2];
        rig.brace = Some(Brace {
            dir,
            t: 0.0,
            wall,
            top,
            surface,
            hands: [0u8, 1].map(|s| which[s as usize].then(|| Hand::new(s))),
        });
    }
    // Over a waist-high obstacle it folds over (its hips pivot on the top); a wall it crumples
    // against.
    let fold = top.filter(|_| kind == Some(obstacle::Kind::Waist));
    if v > limit || !arms {
        let point = bone_of(rig, Role::Pelvis, 2)
            .map(|i| read3(e, rig.bones[i].node + WORLD_TRANSLATION))
            .unwrap_or([0.0; 3]);
        rig.fall = Some(Fall {
            dir,
            speed: if fold.is_some() { v } else { v * 0.5 },
            aggressor,
            point,
            t: 0.0,
            lead: BRACE_REACH + 0.1,
            windmill: false,
            fold,
        });
    }
    let armed = rig.armed;
    drop(rigs);
    room_event(
        e,
        actor,
        "brace",
        &format!(
            "\"on\":{},\"arms\":{arms},\"hands\":[{},{}],\"armed\":{armed},\"wall\":{},\"top\":{},\"speed\":{},\"limit\":{},\"falls\":{},\"fold\":{}",
            room_events::text(kind.map_or("wall", |k| k.name())),
            which[0],
            which[1],
            wall.map_or("null".into(), room_events::num),
            top.map_or("null".into(), room_events::num),
            room_events::num(v),
            room_events::num(limit),
            v > limit || !arms,
            fold.is_some()
        ),
    );
    note(format!(
        "{}: braces against a {} with {}{}",
        describe(e, actor),
        kind.map_or("wall", |k| k.name()),
        match (arms, which, armed) {
            (false, _, _) => "no arm to brace with",
            (true, [true, false], true) => "its free (left) hand, the weapon kept on its aim",
            (true, [true, true], true) => "both hands, the weapon kept in hand",
            (true, [true, false], false) => "its left hand",
            (true, [false, true], _) => "its right hand",
            _ => "its hands",
        },
        if (v > limit || !arms) && fold.is_some() {
            format!(": the push ({v:.2} m/s) is too much, it folds over it into its ragdoll")
        } else if v > limit || !arms {
            format!(": the push ({v:.2} m/s) is too much, it crumples into its ragdoll")
        } else {
            String::new()
        }
    ));
}

/// The way the actor faces (world, flat): its 3D root's forward (+Y) axis.
fn facing(e: &Engine, rig: &Rig) -> V3 {
    if rig.root == 0 {
        return [0.0; 3];
    }
    let r = read9(e, rig.root + WORLD_ROTATION);
    let f = [r[1], r[4], 0.0];
    if f.iter().all(|x| x.is_finite()) {
        unit(f).unwrap_or([0.0; 3])
    } else {
        [0.0; 3]
    }
}

/// Stage 3's brace for a frame (`ragdoll_core::brace`, as the lab's `brace_frame`): each bracing
/// hand's path, plant and release from the arm as it was shown (the bones' last write), and the
/// arm's motor targets reaching where the shown hand goes (from the shoulder, carried onto the
/// animation's `anim`).
fn brace_frame(e: &mut Engine, rig: &mut Rig, anim: &[(M3, V3)], out: &mut [Option<M3>], dt: f32) {
    let Some(mut b) = rig.brace.take() else {
        return;
    };
    b.t += dt;
    let com = rig.body.as_ref().map_or(0.0, |body| body.com_speed());
    let let_go = hands::recovered(rig.balance.is_some(), b.t, com);
    let forward = facing(e, rig);
    let mut said = Vec::new();
    for side in 0..2u8 {
        let Some(hand) = b.hands[side as usize].as_mut() else {
            continue;
        };
        let (Some(u), Some(f), Some(h)) = (
            bone_of(rig, Role::UpperArm, side),
            bone_of(rig, Role::Forearm, side),
            bone_of(rig, Role::Hand, side),
        ) else {
            continue;
        };
        let shown = |i: usize| read3(e, rig.bones[i].node + WORLD_TRANSLATION);
        let (shoulder, elbow, wrist) = (shown(u), shown(f), shown(h));
        let reach = length(sub(elbow, shoulder)) + length(sub(wrist, elbow));
        let target = brace_target(anim[u].1, b.dir, b.wall, b.top);
        let free = rig.free_wrist[side as usize].unwrap_or(wrist);
        let home = home_of(rig, anim, side, |i| world_of(e, rig, i)).map_or(free, |x| x.1);
        let name = if side == 0 { "left" } else { "right" };
        match hand.update(
            b.surface.as_ref(),
            wrist,
            home,
            free,
            shoulder,
            reach,
            target,
            b.t,
            dt,
            let_go,
        ) {
            Some(hands::Event::Planted(at)) => said.push((
                format!(
                    "{name} hand plants at ({:.0}, {:.0}, {:.0})",
                    at[0], at[1], at[2]
                ),
                format!(
                    "\"hand\":\"{name}\",\"planted\":[{},{},{}]",
                    room_events::num(at[0]),
                    room_events::num(at[1]),
                    room_events::num(at[2])
                ),
            )),
            Some(hands::Event::Released(why)) => said.push((
                format!("{name} hand lets go: {why}"),
                format!(
                    "\"hand\":\"{name}\",\"released\":{}",
                    room_events::text(why)
                ),
            )),
            None => {}
        }
        let (goal, w) = match hand.motor_goal() {
            Some(g) => (add(anim[u].1, sub(g, shoulder)), hand.plant_weight()),
            None => (target, hand.reach_weight(b.t)),
        };
        let hand0 = anim[h].1;
        let to = add(hand0, scale(sub(goal, hand0), w.clamp(0.0, 1.0)));
        if let Some((r1, r2)) = reach_arm_ik(anim[u].1, anim[f].1, hand0, to, forward, side) {
            out[u] = Some(r1);
            out[f] = Some(r2);
        }
    }
    for (line, fields) in said {
        note(format!("{}: {line}", describe(e, rig.actor)));
        room_event(e, rig.actor, "brace_hand", &fields);
    }
    if b.hands.iter().all(|h| h.map_or(true, |h| h.done())) {
        note(format!("{}: lets go of the wall", describe(e, rig.actor)));
        return;
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
    let across = unit(cross([0.0, 0.0, 1.0], f.dir)).unwrap_or([1.0, 0.0, 0.0]);
    // It goes over physically (the support fades over the lead, [`apply`]); a windmill is the
    // one pose it is given.
    for side in 0..2u8 {
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
    if f.t - dt < f.lead {
        // The switch into the ragdoll is never made here, at the draw's set-up: the game's task
        // queue is in use then and `KnockExplosion` would only queue it (779cd5b). It is made at
        // the main thread's next update (`crate::knockdown::frame`); the body stays over until
        // the knock state changes (this rig then goes) or the knock-down gives up.
        note(format!(
            "{}: into its ragdoll after {:.2} s of going over{} ({:.2} m/s along ({:.2}, {:.2})): \
             the switch at the next update",
            describe(e, rig.actor),
            f.t,
            if f.windmill { ", windmilling" } else { "" },
            f.speed,
            f.dir[0],
            f.dir[1]
        ));
        // Folding over: launched up enough that the hips clear the top (`fold_launch`).
        let (dir, speed) = match f.fold {
            Some(top) => {
                let pelvis_z = bone_of(rig, Role::Pelvis, 2)
                    .map_or(top, |i| read3(e, rig.bones[i].node + WORLD_TRANSLATION)[2]);
                obstacle::fold_launch(f.dir, f.speed, pelvis_z, top, actor_scale(e, rig.actor))
            }
            None => (f.dir, f.speed),
        };
        crate::knockdown::knock_down_next_update(
            rig.actor,
            rig.id,
            f.aggressor,
            f.point,
            dir,
            speed,
        );
    }
    if f.t > f.lead + FALL_WAIT {
        note(format!(
            "{}: the fall pose let go: still not in its ragdoll {FALL_WAIT} s after the switch \
             was asked (knock state 0)",
            describe(e, rig.actor)
        ));
        rig.shift = [0.0; 3];
        return false;
    }
    rig.fall = Some(f);
    false
}

/// How long a body stays going over waiting for the switch into its ragdoll (s).
const FALL_WAIT: f32 = 1.0;

/// The knock-down a fall asked for did not happen (`crate::knockdown`): the body stands back up
/// from its fall pose.
pub fn fall_failed(actor: u32) {
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    for r in rigs.iter_mut().filter(|r| r.actor == actor) {
        if r.fall.take().is_some() {
            r.shift = [0.0; 3];
        }
    }
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
    if let Some(why) = blocked(e, actor) {
        note(format!(
            "{}: no fall pose: {why}; straight into the ragdoll",
            describe(e, actor)
        ));
        return false;
    }
    if !ensure_rig(e, actor) {
        note(format!(
            "{}: no fall pose: no rig bones; straight into the ragdoll",
            describe(e, actor)
        ));
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
    // A waist-high obstacle close behind: it folds over it (`obstacle::FOLD_REACH`).
    let pelvis =
        bone_of(rig, Role::Pelvis, 2).map(|i| read3(e, rig.bones[i].node + WORLD_TRANSLATION));
    drop(rigs);
    let fold = pelvis.and_then(|at| {
        let floor_z = ground_point(e, actor, at).map_or(at[2] - 60.0, |g| g[2]);
        let scale_ = actor_scale(e, actor);
        let reach = obstacle::FOLD_REACH * scale_ * UNITS_PER_METRE;
        obstacle::scan(&mut GameWorld { e, actor }, at, floor_z, dir, reach, scale_)
            .filter(|o| o.kind == obstacle::Kind::Waist)
            .map(|o| o.top)
    });
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(rig) = rigs.iter_mut().find(|r| r.actor == actor && r.root == root) else {
        return false;
    };
    if rig.fall.is_some() {
        return true;
    }
    rig.fall = Some(Fall {
        dir,
        speed,
        aggressor,
        point,
        t: 0.0,
        lead: FALL_LEAD,
        windmill: false,
        fold,
    });
    rig.since = Instant::now();
    drop(rigs);
    room_event(
        e,
        actor,
        "fall",
        &format!(
            "\"speed\":{},\"fold\":{}",
            room_events::num(speed),
            fold.is_some()
        ),
    );
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
    crate::one_hand::clear();
    *LAST.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// A room event about `actor` ([`room_events`]: the Linux room's scripted runs), with its form id.
fn room_event(e: &Engine, actor: u32, kind: &str, fields: &str) {
    if room_events::on() {
        room_events::record(
            kind,
            &format!("\"actor\":\"{:08X}\",{fields}", e.mem.u32(actor + FORM_ID)),
        );
    }
}

fn describe(e: &Engine, actor: u32) -> String {
    format!("Hit reaction: {:08X}", e.mem.u32(actor + FORM_ID))
}

/// The log lines, at most 20 a second.
fn note(line: String) {
    room_events::record("note", &format!("\"text\":{}", room_events::text(&line)));
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
