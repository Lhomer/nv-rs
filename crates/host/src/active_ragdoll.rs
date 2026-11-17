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
    /// The physical body's origin (world, game units: the animated pelvis where the body was
    /// made). The body runs in metres from it, not from the world's origin thousands of metres
    /// away, where f32 is coarse enough to shake it.
    pub origin: V3,
    /// A dropped body's way back to the animation (`ragdoll_core::active::Fade`), and the last
    /// written pose's offsets over the animation it starts from (each bone's, the pelvis's).
    pub fade: Option<active::Fade>,
    pub last: Option<(Vec<active::Q>, V3)>,
    /// The steps' pelvis offset when they ended, eased out from then ([`SHIFT_BACK`]), and after a
    /// fall pose that did not go into the ragdoll, the support it had and since when it eases
    /// back ([`FALL_BACK`]).
    pub shift_back: Option<(V3, Instant, f32)>,
    pub support_back: Option<(f32, Instant)>,
    /// A step's move of the reference (`crate::knockdown::place`) not yet in the root's world:
    /// the root's world before it, the move, since when.
    pub placed: Option<(V3, V3, Instant)>,
    /// The base reads since the body was made (`ragdoll_core::pose::read_base`): how many times a
    /// later pass moved a held bone and the most it moved one (in tolerances), how many times an
    /// animated local jumped and was eased in (`pose::capped`).
    pub drift: u32,
    pub drift_most: f32,
    pub jumps: u32,
    /// This frame's pelvis offset written (game units) and the body's most spread (a bone's
    /// distance from the pelvis, m): for the clips' samples ([`sample_fields`]).
    pub pelvis_now: f32,
    pub spread_now: f32,
    /// The game's own moves of the actor (its AI walking it) carried by the reaction ([`carry`]):
    /// the reference the frame before, and the steps' moves of it since (not the game's).
    pub carried: Option<V3>,
    pub own_moved: V3,
    /// The hand-back only settles (`ragdoll_core::active::Settle`).
    pub settle: active::Settle,
    /// For the clips' samples ([`sample_fields`]): the shown pelvis (world) and its yaw against
    /// the animation the frame before; this frame's yaw rate (rad/s), the shown pelvis's move
    /// beyond the actor's own (game units) and whether the hand-back holds it.
    pub shown_before: Option<(V3, f32)>,
    pub yaw_rate_now: f32,
    pub jump_now: f32,
    /// The steps' own moves of the actor since the body was made (left out of its carry), and
    /// each bone's animated frame the frame before (world rotation, joint less those moves): the
    /// body carried along with the animation's own motion (`active::Body::carry`).
    pub own_total: V3,
    pub carry_from: Option<Vec<(active::Q, V3)>>,
    /// The shown offsets held to the push (`active::Limit`), the push (m/s, the most of the
    /// body's hits'), the body's weight in the shown pose last frame.
    pub limit: active::Limit,
    pub react_v: f32,
    pub shown_w: f32,
    /// When the reaction yielded to its animation (`active::yields`), the AI's move (units) and
    /// turn (rad) of the actor since the last hit, its facing the frame before, and how fast the
    /// AI moves (units/s) and turns (rad/s) it now (eased).
    pub yield_at: Option<Instant>,
    pub moved_since_hit: f32,
    pub turned_since_hit: f32,
    pub facing_before: Option<V3>,
    pub ai_speed: f32,
    pub ai_turn: f32,
    /// For the clips' samples: each bone's shown offset from its animation the frame before,
    /// this frame's fastest change of one (rad/s) and which.
    pub offsets_before: Vec<active::Q>,
    pub spin_now: f32,
    pub spin_bone: Role,
    /// How many offsets `active::Limit` has held since the body was made (the pose log's).
    pub limited: u32,
    /// What the frame was drawn with ([`after_draw`]): the pelvis's, the spine's and the head's
    /// nodes and their animated world (rotation, position) this frame; the most any was drawn off
    /// it (game units, degrees) this frame and over the reaction.
    pub drawn_ref: Vec<(u32, M3, V3)>,
    pub drawn_offset: f32,
    pub drawn_turn: f32,
    pub drawn_most: (f32, f32),
    /// Frames the body was shown off its animation but drawn on it (the write not reaching the
    /// drawing), and whether that was said.
    pub drawn_missing: u32,
    pub drawn_warned: bool,
    /// What the world pass drew this frame ([`read_drawn`]: offset, turn, the checked bones found
    /// in the skin, how many of them the rig's own nodes, the skin's bones), and the last frame's
    /// count of those (the pose log's `drawn_skin`).
    pub drawn_seen: Option<(f32, f32, u32, u32, u32)>,
    pub drawn_skin: (u32, u32, u32),
}

/// How much deeper than the animation (game units) a shown forearm or hand may be in the torso
/// before its arm is turned out (`active::Body::clear_arms`).
const ARM_SLACK: f32 = 0.35;
/// How long the steps' pelvis offset takes back to none once they are done (s).
const SHIFT_BACK: f32 = 0.3;
/// How long a step's move of the reference may wait for the root's world to show it (s).
const PLACED_WAIT: f32 = 0.5;

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
    /// The actor's facing when the brace began, and how far the game walked it since: its hands
    /// let go once it turns or walks it away (`ragdoll_core::brace::let_go`).
    pub facing: V3,
    pub walked: f32,
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
    /// What the bone held when it was read this frame (the game's own value: put back after the
    /// draw, [`unwrite`]).
    pub raw: Option<(M3, V3)>,
    /// The rig bone it hangs from, and (an arm's) its shown pose offset following the poses it is
    /// given ([`Follow`]).
    pub parent: Option<usize>,
    pub follow: Follow,
    /// The nodes between its rig parent and it (top down; none of them written), and (without a
    /// rig parent) the node it hangs from: the animated world under it is worked out from them
    /// (`ragdoll_core::pose`), never read back from what the layer wrote.
    pub between: Vec<u32>,
    pub above: u32,
    /// Its parent in the physical body (`ragdoll_core::active::physics_parents`): its rig parent,
    /// or the pelvis for a bone that reaches none (FNV's `Bip01 Spine`, under `Bip01 NonAccum`
    /// beside the pelvis). The body, its targets and the shown joints follow it; the write follows
    /// the node tree (`parent`, `above`, `between`), the bone's shown joint carried by a shift.
    pub phys: Option<usize>,
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
                    raw: None,
                    parent: None,
                    follow: Follow::REST,
                    between: Vec::new(),
                    above: 0,
                    phys: None,
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
    if let Some(pelvis) = found.iter().position(|b| b.role == Role::Pelvis) {
        let node: Vec<Option<usize>> = found.iter().map(|b| b.parent).collect();
        for (bone, phys) in found.iter_mut().zip(active::physics_parents(&node, pelvis)) {
            bone.phys = phys;
        }
    } else {
        for bone in &mut found {
            bone.phys = bone.parent;
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
        origin: [0.0; 3],
        fade: None,
        last: None,
        shift_back: None,
        support_back: None,
        placed: None,
        drift: 0,
        drift_most: 0.0,
        jumps: 0,
        pelvis_now: 0.0,
        spread_now: 0.0,
        carried: None,
        own_moved: [0.0; 3],
        settle: active::Settle::default(),
        shown_before: None,
        yaw_rate_now: 0.0,
        jump_now: 0.0,
        own_total: [0.0; 3],
        carry_from: None,
        limit: active::Limit::default(),
        react_v: 0.0,
        shown_w: 0.0,
        yield_at: None,
        moved_since_hit: 0.0,
        turned_since_hit: 0.0,
        facing_before: None,
        ai_speed: 0.0,
        ai_turn: 0.0,
        offsets_before: Vec::new(),
        spin_now: 0.0,
        spin_bone: Role::Pelvis,
        limited: 0,
        drawn_ref: Vec::new(),
        drawn_offset: 0.0,
        drawn_turn: 0.0,
        drawn_most: (0.0, 0.0),
        drawn_missing: 0,
        drawn_warned: false,
        drawn_seen: None,
        drawn_skin: (0, 0, 0),
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
    // A hit while it is handed back: a fresh body from this pose, the pose shown eased out over
    // it (its weight jumped back to full over a body left behind: a hand 43 rad/s in one frame
    // in the lab).
    if rig.body.is_some() && rig.shown_w < 0.999 {
        rig.body = None;
        if let Some((off, p)) = rig.last.take() {
            rig.fade = Some(active::Fade::new(off, p));
        }
    }
    rig.react_v = if rig.body.is_some() {
        rig.react_v.max(v)
    } else {
        v
    };
    rig.yield_at = None;
    rig.moved_since_hit = 0.0;
    rig.turned_since_hit = 0.0;
    rig.since = Instant::now();
    rig.armed = armed;
    rig.aim_hold = aim_hold;
    let spread = spread_of(v);
    // The struck bone: the rig's bone whose segment the impact is on.
    let positions: Vec<V3> = rig
        .bones
        .iter()
        .map(|b| read3(e, b.node + WORLD_TRANSLATION))
        .collect();
    let parents: Vec<Option<usize>> = rig.bones.iter().map(|b| b.parent).collect();
    let struck = ragdoll_core::flinch::struck_bone(&positions, &parents, point);
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

/// The bones written this frame and the game's own value of each (what it held when read).
static WRITTEN: Mutex<Vec<(u32, (M3, V3))>> = Mutex::new(Vec::new());

/// After the draw (from `crate::gunplay`'s `FIRST_PERSON_SETUP` wrapper): every bone written
/// this frame given back the game's own local. The reaction is drawn (the world transforms the
/// write updated stay until the game updates them), but nothing of it is left in the bones for
/// the game's next frame: its animation writes over its own pose, a later pass moves its own
/// pose, and the next read is the animation as it is, never our write taken for it (b44a964
/// left the write in the bones: a walk moving a bone the way its offset went was read as a
/// later pass's move of the write and kept stale until 12.65 units off, then jumped; a pass
/// moving the write past that was taken for the animation, the offset put on itself: Patrick's
/// 14-16-02 clip, the upper body spinning round the waist and a snap at the end).
pub fn unwrite(e: &mut Engine) {
    let written = std::mem::take(&mut *WRITTEN.lock().unwrap_or_else(|p| p.into_inner()));
    for (node, (rot, t)) in written {
        if node != 0 {
            write9(e, node + LOCAL_ROTATION, &rot);
            write3(e, node + LOCAL_TRANSLATION, t);
        }
    }
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    for rig in rigs.iter_mut() {
        for bone in rig.bones.iter_mut() {
            bone.written = None;
        }
    }
}

/// The frame drawn by `draw` (the game's `DRAW_FRAME`, `crate::fixes::install`), then what it
/// was drawn with read ([`after_draw`]) and the write put back ([`unwrite`]), also when the
/// drawing unwinds. The write stays in the bones through the whole frame's drawing: the world is
/// drawn after the first-person set-up where it is written, and put back there (d0f4192) the
/// reactions were never seen (Patrick's 15-16-34 and 15-18-15 clips: bodies active, offsets up
/// to 9 units, the actors drawn in their plain idle).
pub fn draw_then_put_back(e: &mut Engine, draw: impl FnOnce(&mut Engine)) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| draw(e)));
    after_draw(e);
    unwrite(e);
    if let Err(unwinding) = result {
        std::panic::resume_unwind(unwinding);
    }
}

/// The frame drawn by `draw` (the game's `DRAW_FRAME`, `crate::fixes::install`) with the
/// reactions in it: written first ([`apply`]: the bones' locals and their world transforms), so
/// the world pass (`00873200`, before the first-person set-up) culls, skins and draws the actors
/// from them; what it drew with read there ([`world_pass`]); then put back ([`unwrite`]), also
/// when the drawing unwinds, before the game's next animation update. 00262dd wrote them at the
/// first-person set-up (`00874c10`), after the world pass: never drawn (Patrick's 18-21-05 clip).
pub fn around_draw(e: &mut Engine, draw: impl FnOnce(&mut Engine)) {
    if !crate::gunplay::is_on_now() {
        draw(e);
        return;
    }
    around_draw_with(e, apply, draw);
}

/// [`around_draw`] with its write `write` (the tests' own).
fn around_draw_with(
    e: &mut Engine,
    write: impl FnOnce(&mut Engine),
    draw: impl FnOnce(&mut Engine),
) {
    write(e);
    DRAWN_PENDING.store(true, std::sync::atomic::Ordering::Relaxed);
    draw_then_put_back(e, draw);
    DRAWN_PENDING.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// `Main`'s world pass (`00873200`): where the actors are culled, skinned and drawn ([`world_pass`]).
pub const WORLD_PASS: u32 = 0x0087_3200;

/// The world pass's first call this frame is still to be read ([`world_pass`]).
static DRAWN_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The world pass (`00873200`, `Main`'s world accumulate and draw; `crate::fixes::install`
/// registers it with the joined drawing): at its first call of the frame, what each reacting
/// actor is drawn with read from the bones its skinned geometry lists ([`read_drawn`]), then the
/// pass as the game's.
pub fn world_pass(e: &mut Engine, a: &[u32]) -> Ret {
    if DRAWN_PENDING.swap(false, std::sync::atomic::Ordering::Relaxed) {
        read_drawn(e);
    }
    engine::units::fallout_misc::main_p2::fn_00873200(
        e,
        Ptr::new(a[0]),
        a[1],
        a[2],
        a[3] as u8,
        a[4] as u8,
    );
    Ret::default()
}

/// An `NiAVObject`'s `IsNode` (vtable `+0x0c`, the node or 0) and `IsGeometry` (`+0x18`), an
/// `NiGeometry`'s skin instance (`+0xbc`), a skin instance's skin data (`+0x08`) and bone array
/// (`+0x14`), a skin data's bone count (`+0x44`).
const IS_NODE_SLOT: u32 = 0x0c;
const IS_GEOMETRY_SLOT: u32 = 0x18;
const GEOMETRY_SKIN: u32 = 0xbc;
const SKIN_DATA: u32 = 0x08;
const SKIN_BONES: u32 = 0x14;
const SKIN_DATA_BONES: u32 = 0x44;
/// The most objects, bones a skin, and the deepest a walk of an actor's 3D looks at.
const MOST_OBJECTS: usize = 1024;
const MOST_SKIN_BONES: u32 = 256;
const MOST_DEPTH: u32 = 16;

/// The bone nodes the skinned geometries under `root` list (their skin instances' bone arrays):
/// what the drawing poses the actor's skin with.
fn skin_bones(e: &mut Engine, root: u32) -> Vec<u32> {
    let mut bones = Vec::new();
    let mut stack = vec![(root, 0u32)];
    let mut seen = 0usize;
    while let Some((object, depth)) = stack.pop() {
        if object == 0 || seen >= MOST_OBJECTS {
            continue;
        }
        seen += 1;
        if e.vcall(object, IS_NODE_SLOT, &args![]).u32() != 0 {
            if depth >= MOST_DEPTH {
                continue;
            }
            let count = e.call(CHILD_COUNT, &args![object]).u32().min(256);
            for i in 0..count {
                let child = e.call(CHILD_AT, &args![object, i]).u32();
                stack.push((child, depth + 1));
            }
            continue;
        }
        if e.vcall(object, IS_GEOMETRY_SLOT, &args![]).u32() == 0 {
            continue;
        }
        let skin = e.mem.u32(object + GEOMETRY_SKIN);
        if skin == 0 {
            continue;
        }
        let data = e.mem.u32(skin + SKIN_DATA);
        let array = e.mem.u32(skin + SKIN_BONES);
        if data == 0 || array == 0 {
            continue;
        }
        let count = e.mem.u32(data + SKIN_DATA_BONES).min(MOST_SKIN_BONES);
        for i in 0..count {
            let bone = e.mem.u32(array + 4 * i);
            if bone != 0 && !bones.contains(&bone) {
                bones.push(bone);
            }
        }
    }
    bones
}

/// A node's name (`m_kName`, `+0x08`), empty for none.
fn node_name(e: &Engine, node: u32) -> Vec<u8> {
    let name = e.mem.u32(node + NODE_NAME);
    if name == 0 {
        Vec::new()
    } else {
        e.mem.cstr(name)
    }
}

/// What the drawing sees of `refs` (the rig's nodes and their animated world): each matched by
/// name to one of the skin's `bones`, that bone's world transform against the animated one.
/// Answers the most offset (game units) and turn (degrees), how many matched, and how many of
/// those were the rig's own node.
fn drawn_against(e: &mut Engine, bones: &[u32], refs: &[(u32, M3, V3)]) -> (f32, f32, u32, u32) {
    let names: Vec<(u32, Vec<u8>)> = bones.iter().map(|&b| (b, node_name(e, b))).collect();
    let (mut most, mut turn, mut matched, mut same) = (0.0f32, 0.0f32, 0u32, 0u32);
    for &(node, anim_r, anim_p) in refs {
        let want = node_name(e, node);
        let Some(&(bone, _)) = names
            .iter()
            .find(|(_, n)| !want.is_empty() && n.eq_ignore_ascii_case(&want))
        else {
            continue;
        };
        let p = read3(e, bone + WORLD_TRANSLATION);
        let r = read9(e, bone + WORLD_ROTATION);
        if !(p.iter().all(|x| x.is_finite()) && r.iter().all(|x| x.is_finite())) {
            continue;
        }
        matched += 1;
        same += u32::from(bone == node);
        most = most.max(length(sub(p, anim_p)));
        let d = mul(&r, &transpose(&anim_r));
        turn = turn.max(length(ragdoll_core::flinch::rotation_vector(&d)).to_degrees());
    }
    (most, turn, matched, same)
}

/// What each reacting actor is drawn with, as the world pass begins ([`world_pass`]): its pelvis,
/// spine top and head as its skinned geometry's bones have them, against their animated world
/// (`Rig::drawn_offset`, `drawn_turn`; the pose log's).
fn read_drawn(e: &mut Engine) {
    let wanted: Vec<(u32, u32, Vec<(u32, M3, V3)>)> = {
        let rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
        rigs.iter()
            .filter(|r| !r.drawn_ref.is_empty())
            .map(|r| (r.actor, r.root, r.drawn_ref.clone()))
            .collect()
    };
    let mut seen = Vec::new();
    for (actor, root, refs) in wanted {
        let bones = skin_bones(e, root);
        seen.push((actor, drawn_against(e, &bones, &refs), bones.len()));
    }
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    for (actor, (most, turn, matched, same), skin) in seen {
        if let Some(rig) = rigs.iter_mut().find(|r| r.actor == actor) {
            rig.drawn_seen = Some((most, turn, matched, same, skin as u32));
        }
    }
}

/// What each reacting actor was drawn with this frame ([`read_drawn`] at the world pass; nothing
/// for a frame without one): `Rig::drawn_offset`, `drawn_turn`, the most over the reaction, and a
/// reaction shown off its animation but drawn on it counted and said ([`DRAWN_MISSING_SAID`]).
pub fn after_draw(e: &mut Engine) {
    let _ = e;
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    for rig in rigs.iter_mut() {
        if rig.drawn_ref.is_empty() {
            continue;
        }
        rig.drawn_ref.clear();
        let Some((most, turn, matched, same, skin)) = rig.drawn_seen.take() else {
            continue;
        };
        rig.drawn_offset = most;
        rig.drawn_turn = turn;
        rig.drawn_skin = (matched, same, skin);
        rig.drawn_most = (rig.drawn_most.0.max(most), rig.drawn_most.1.max(turn));
        // Shown well off its animation, drawn on it: the write is lost before the drawing.
        if rig.pelvis_now > 1.0 && most < 0.05 && turn < 0.05 {
            rig.drawn_missing += 1;
            if rig.drawn_missing >= DRAWN_MISSING_SAID && !rig.drawn_warned {
                rig.drawn_warned = true;
                note(format!(
                    "{:08X}: the reaction's write did not reach the drawing ({} frames shown {:.1} \
                     units off its animation, its skin's bones on it; {matched} of the checked \
                     bones found in its skin, {same} of them the rig's own nodes, {skin} bones)",
                    rig.id, rig.drawn_missing, rig.pelvis_now
                ));
            }
        }
    }
}

/// Frames a reaction shown off its animation but drawn on it before it is said.
const DRAWN_MISSING_SAID: u32 = 30;

/// The reaction yields to its animation now (`active::YIELD_BACK`): its body handed back
/// quickly, a brace's hands let go, the steps end. Once a hit.
fn yield_now(rig: &mut Rig, why: &str) {
    if rig.yield_at.is_some() || rig.body.is_none() || rig.fall.is_some() {
        return;
    }
    rig.yield_at = Some(Instant::now());
    if let Some(s) = rig.balance.as_mut() {
        s.recovering = true;
    }
    note(format!(
        "{:08X}: yields to its animation ({why}): handed back over {} s",
        rig.id,
        active::YIELD_BACK
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
    let mut written_now: Vec<(u32, (M3, V3))> = Vec::new();
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
        // off, or a later pass moved it) the pose the last write was over
        // (`ragdoll_core::pose::read_base`): a move that is not the animation's is never taken
        // for it (that put the offset on top of itself every frame: the body leaned away, then
        // snapped back when the animation wrote the bone). While the body reacts or fades, an
        // animated local's jump is eased in (`pose::capped`), said once.
        let reacting = rig.body.is_some() || rig.fade.is_some();
        let k = (dt * 60.0).clamp(0.5, 3.0);
        let mut jumped: Option<(Role, f32)> = None;
        let jumps_before = rig.jumps;
        for bone in &mut rig.bones {
            let now = (
                read9(e, bone.node + LOCAL_ROTATION),
                read3(e, bone.node + LOCAL_TRANSLATION),
            );
            let before = bone.pose;
            bone.raw = Some(now);
            // The write is put back after the draw ([`unwrite`]): what the bone holds is the
            // game's own pose (never our write, never stale).
            let (base, how) = pose::read_base(now, bone.pose, bone.written);
            match (how, before) {
                (pose::Read::Disturbed, _) => {
                    rig.drift += 1;
                    if let Some(w) = bone.written {
                        rig.drift_most = rig.drift_most.max(pose::apart(&now, &w));
                    }
                }
                (pose::Read::Animated, Some(before)) if reacting => {
                    // A jump of its animation (a new set: a cripple's): the reaction yields to it
                    // (not eased in under the reaction).
                    let (_, cut) = pose::capped(&before, &base, k);
                    if cut {
                        rig.jumps += 1;
                        jumped.get_or_insert((bone.role, pose::apart(&before, &base)));
                    }
                }
                _ => {}
            }
            bone.pose = Some(base);
        }
        if let (Some((role, by)), 0) = (jumped, jumps_before) {
            yield_now(&mut rig, "its animation jumped");
            note(format!(
                "{}: its animation jumped under the reaction ({} {by:.0} tolerances in a frame): \
                 the reaction yields to it",
                describe(e, rig.actor),
                BONES
                    .iter()
                    .find(|(_, r)| *r == role)
                    .map_or("a bone", |b| b.0),
            ));
        }
        // The animated pose in the world, worked out from the animation's own locals down from
        // the actor's root (never read back from the bones, which may hold the last write), and
        // each bone's parent node's animated world.
        let (mut anim, mut parents) = animated(e, &rig);
        let Some(pi) = bone_of(&rig, Role::Pelvis, 2) else {
            continue;
        };
        // A dropped body's way back to the animation goes on (`ragdoll_core::active::Fade`).
        if let Some(fade) = rig.fade.as_mut() {
            if !fade.advance(dt) {
                rig.fade = None;
            }
        }
        // The physical body: made the frame after a hit, from this pose and the last (its
        // motion), then the hits go in. Only a hit makes one: never the end of a fade, nor a rig
        // left without one (a body made with no hit drifted and was dropped again, about 2 s a
        // reaction: Patrick's 126915e clip). The fade is written while a hit's body waits.
        if rig.body.is_none() && !rig.pending.is_empty() {
            match rig.before.take() {
                Some(before) if before.len() == anim.len() && dt > 0.0 => {
                    rig.origin = anim[pi].1;
                    rig.drift = 0;
                    rig.drift_most = 0.0;
                    rig.jumps = 0;
                    let mut body = make_body(e, &rig, &anim, &before, dt);
                    body.unarmed(!rig.armed);
                    rig.body = Some(body);
                    rig.own_total = [0.0; 3];
                    rig.carry_from = None;
                    rig.limit.open();
                    rig.settle.open();
                    rig.limited = 0;
                }
                _ => {
                    rig.before = Some(anim.clone());
                    if rig.fade.is_none() {
                        keep.push(rig);
                        continue;
                    }
                }
            }
        }
        // The game's own move of the actor this frame (its AI walking it), carried by the
        // reaction ([`carry`]).
        let moved_by_game = carry(e, &mut rig, dt);
        // World (game units) to the body's metres from its origin.
        let origin = rig.origin;
        let to_body = move |p: V3| scale(sub(p, origin), 1.0 / UNITS_PER_METRE);
        if let Some(body) = rig.body.as_mut() {
            for h in std::mem::take(&mut rig.pending) {
                body.hit(h.p, h.dir, to_body(h.at), h.spread, &h.dips, &h.takes);
            }
        }
        // The steps read the animated pose (as the lab does), not the shown one, which holds the
        // last write. A step's move of the reference that the root's world does not show yet is
        // carried by the pelvis (no jump back for a frame).
        let mut root_at = read3(e, root + WORLD_TRANSLATION);
        let placed_before = rig.placed.map(|p| p.2);
        let mut overrides = balance_frame(e, &mut rig, &anim, root_at, dt);
        if rig.placed.map(|p| p.2) != placed_before {
            // A step moved the reference: if the root's world shows it once brought up to date,
            // the animated pose is worked out again from there (as the lab's is).
            update(e, root);
            let now = read3(e, root + WORLD_TRANSLATION);
            if length(sub(now, root_at)) > 0.5 {
                root_at = now;
                (anim, parents) = animated(e, &rig);
            }
        }
        let lag = match rig.placed {
            Some((was, moved, since)) => {
                let seen = sub(root_at, was);
                let lag = sub(moved, seen);
                if length(lag) < 1.0 || since.elapsed().as_secs_f32() > PLACED_WAIT {
                    rig.placed = None;
                    [0.0; 3]
                } else {
                    lag
                }
            }
            None => [0.0; 3],
        };
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
            let up = rig.bones[b].phys.map_or(IDENTITY, |p| chain[p]);
            chain[b] = match overrides[b] {
                Some(o) => mul(&up, &o),
                None => up,
            };
            target[b] = mul(&chain[b], &anim[b].0);
        }
        // The steps' pelvis offset, eased out once they are done (or the fall that held it is
        // over).
        if rig.balance.is_none() && rig.fall.is_none() {
            if rig.shift_back.is_none() && length(rig.shift) > 0.0 {
                rig.shift_back = Some((rig.shift, Instant::now(), SHIFT_BACK));
            }
            rig.shift = match rig.shift_back {
                Some((from, since, over)) => {
                    let k = active::ease(since.elapsed().as_secs_f32() / over);
                    if k >= 1.0 {
                        rig.shift_back = None;
                    }
                    scale(from, 1.0 - k)
                }
                None => rig.shift,
            };
        }
        let pelvis_target = add(add(anim[pi].1, rig.shift), lag);
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
        let support = match (&rig.fall, rig.support_back) {
            (Some(f), _) => fall_support(f),
            (None, Some((from, since))) => {
                let k = active::ease(since.elapsed().as_secs_f32() / FALL_BACK);
                if k >= 1.0 {
                    rig.support_back = None;
                }
                from + (1.0 - from) * k
            }
            (None, None) => 1.0,
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
        let mut shown: Vec<M3> = target.clone();
        let mut pelvis_offset = add(rig.shift, lag);
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
                    to_body(at),
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
                pelvis: to_body(pelvis_target),
                feet: feet_now.iter().map(|&(i, at)| (i, to_body(at))).collect(),
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
            // Carried along with the animation's own motion (its AI's walk and turn, a new set),
            // the steps' own moves of the actor left out (`Body::carry`); a fall's is physical.
            let frames: Vec<(active::Q, V3)> = anim
                .iter()
                .map(|a| (active::qfrom_m(&a.0), sub(add(a.1, lag), rig.own_total)))
                .collect();
            if let (Some(from), None) = (rig.carry_from.as_ref(), rig.fall.as_ref()) {
                if from.len() == frames.len() {
                    let from: Vec<_> = from.iter().map(|&(q, p)| (q, to_body(p))).collect();
                    let to: Vec<_> = frames.iter().map(|&(q, p)| (q, to_body(p))).collect();
                    body.carry(&from, &to);
                }
            }
            rig.carry_from = Some(frames);
            body.free_arms(brace_now.is_none() && rig.fall.is_none());
            body.step(
                &targets,
                dt,
                active::strength(rig.hit_at.elapsed().as_secs_f32()) * cond.motor_scale(),
            );
            if let Err(why) = body.check() {
                failed = Some(format!("the body: {why}"));
            }
            // Handed back by its time whatever still holds it, unless it goes over
            // (`active::reaction_weight`: Patrick's c2a53c0 fold clip held one hit's body 5.7 s).
            let w = active::reaction_weight(
                since,
                rig.hit_at.elapsed().as_secs_f32(),
                rig.fall.is_some(),
                rig.yield_at.map(|y| y.elapsed().as_secs_f32()),
            );
            phys_w = w;
            let pose = body.pose();
            // The targets as the body tracks them (no elbow or knee past its human range) and the
            // body, blended (`Body::shown`: the elbows and knees against their parents).
            let wanted: Vec<active::Q> = target.iter().map(active::qfrom_m).collect();
            let mut blended = body.shown(&wanted, w);
            let sim = add(scale(pose[pi].1, UNITS_PER_METRE), origin);
            let mut off = scale(sub(sim, pelvis_target), w);
            // Handed back, it only settles (`active::Settle`): never shown gaining on the
            // animation; a brace's arms go home their own way.
            // Its ceilings come down with the weight (never frozen while it turns).
            let bracing = brace_now.is_some();
            let bones = &rig.bones;
            let free = |b: usize| bracing && bones[b].role.is_arm();
            if w < 1.0 && rig.fall.is_none() {
                let keep = if rig.shown_w > 0.0 {
                    w / rig.shown_w
                } else {
                    1.0
                };
                rig.settle
                    .hold_down(&mut blended, &wanted, &mut off, keep, free);
            } else {
                rig.settle.open();
            }
            // Never more than the push justifies, nor a bone turning off its pose faster than a
            // person (`active::Limit`: the clip's upper body spun round the waist); a fall is
            // the ragdoll's, only followed.
            if rig.fall.is_none() {
                let held = rig
                    .limit
                    .hold(&mut blended, &wanted, &mut off, rig.react_v, dt, free);
                rig.limited += held;
            } else {
                rig.limit.follow(&blended, &wanted, off);
            }
            rig.shown_w = w;
            for (b, q) in blended.into_iter().enumerate() {
                shown[b] = active::m_from_q(q);
            }
            pelvis_offset = add(add(rig.shift, lag), off);
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
        // The arms turned out of the torso where they went deeper than the animation has them
        // (`active::Body::clear_arms`: Patrick's 10:47 clip, arms through the body).
        if let Some(body) = rig.body.as_ref() {
            // Not an arm a brace or the grip puts on its goal (that goal is held already).
            let mut skip: Vec<usize> = Vec::new();
            for side in 0..2u8 {
                // A planted brace hand stays where it touched.
                let planted = rig
                    .brace
                    .and_then(|b| b.hands[side as usize])
                    .is_some_and(|h| h.planted());
                // An armed actor's arms are the weapon pose's and its aim's.
                if planted || (side == 0 && rig.grip_hold) || rig.armed {
                    skip.extend(bone_of(&rig, Role::Hand, side));
                }
            }
            let mut at = shown_positions(&rig, &anim, &shown, pi, pelvis_offset);
            body.clear_arms(
                &mut shown,
                &mut at,
                &anim,
                |r, j, i| body.capsule_at(r, j, i, UNITS_PER_METRE),
                ARM_SLACK,
                &skip,
            );
            // And inside the shoulder's human range, against the chest as it is turned.
            // The chest: the last spine bone (the clavicles hang from it).
            if let Some(c) = (0..rig.bones.len())
                .rev()
                .find(|&i| rig.bones[i].role == Role::Spine)
            {
                let chest = mul(&shown[c], &transpose(&anim[c].0));
                body.hold_shoulders(
                    &mut shown,
                    &mut at,
                    apply_m(&chest, forward),
                    apply_m(&chest, [0.0, 0.0, 1.0]),
                    &anim,
                    forward,
                    &skip,
                );
            }
        }
        // What is left of a dropped body's last pose, on top.
        if let Some(fade) = &rig.fade {
            fade.on(&mut shown, &mut pelvis_offset);
        }
        let mut offsets = offsets_of(&rig.bones, &anim, &shown);
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
            // What it looked like (Patrick's 9:06 PM log: the snaps came right after the steps).
            let spread = rig.body.as_ref().map_or(0.0, |b| spread_of_body(b, pi));
            // The body's joints: the most gap and its bone, and the bones it holds as roots (the
            // pelvis alone, unless a bone's physics parent is missing).
            let (gap, gap_bone) = rig.body.as_ref().map_or((0.0, None), |b| b.joint_gap());
            let roots = rig.body.as_ref().map_or_else(Vec::new, |b| b.roots());
            let name = |i: usize| {
                BONES
                    .iter()
                    .find(|(_, r)| *r == rig.bones[i].role)
                    .map_or("a bone", |b| b.0)
            };
            let parents = rig
                .bones
                .iter()
                .enumerate()
                .filter(|(_, b)| b.parent != b.phys)
                .map(|(i, b)| {
                    format!(
                        "{} hangs from {} (node root)",
                        name(i),
                        b.phys.map_or("nothing", name)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let joints = format!(
                "joint gap {:.3} m{}; roots {}{}",
                gap,
                gap_bone
                    .map(|i| format!(" ({})", name(i)))
                    .unwrap_or_default(),
                roots
                    .iter()
                    .map(|&i| name(i))
                    .collect::<Vec<_>>()
                    .join(", "),
                if parents.is_empty() {
                    String::new()
                } else {
                    format!("; {parents}")
                }
            );
            let last_pelvis = rig.last.as_ref().map_or(0.0, |l| length(l.1));
            // Eased back from the pose last shown, unless a fade with no body under it went
            // wrong (the animation itself is not believable): that is put back at once.
            let fade_from = match (&rig.fade, &rig.body) {
                (Some(_), None) => None,
                _ => rig.last.take(),
            };
            note(format!(
                "{}: the active ragdoll is dropped back to the animation: {why} (pelvis offset \
                 {:.1} units, {last_pelvis:.1} last shown; most spread {spread:.2} m; base drift: \
                 {} reads moved by a later pass, the most {:.0} tolerances; {} animation jumps \
                 eased in; {joints}); {}",
                describe(e, rig.actor),
                length(pelvis_offset),
                rig.drift,
                rig.drift_most,
                rig.jumps,
                if fade_from.is_some() {
                    format!("eased back over {} s", active::FADE_BACK)
                } else {
                    "put back at once".to_string()
                }
            ));
            rig.body = None;
            rig.balance = None;
            rig.brace = None;
            rig.fall = None;
            rig.shift = [0.0; 3];
            rig.shift_back = None;
            rig.placed = None;
            rig.pending.clear();
            rig.before = None;
            rig.grip_hold = false;
            let Some((off, p)) = fade_from else {
                // Its bones put back to the animation's pose, then it goes.
                for bone in &mut rig.bones {
                    if let Some((rot, t)) = bone.pose {
                        write9(e, bone.node + LOCAL_ROTATION, &rot);
                        write3(e, bone.node + LOCAL_TRANSLATION, t);
                    }
                }
                update(e, root);
                continue;
            };
            // Never in one frame: the pose last shown, eased out over the animation
            // (`ragdoll_core::active::Fade`).
            let fade = active::Fade::new(off, p);
            shown = anim.iter().map(|a| a.0).collect();
            pelvis_offset = [0.0; 3];
            fade.on(&mut shown, &mut pelvis_offset);
            offsets = offsets_of(&rig.bones, &anim, &shown);
            rig.fade = Some(fade);
        }
        let resting = rig.body.is_none()
            && !busy
            && rig.fade.is_none()
            && rig.shift_back.is_none()
            && rig.placed.is_none();
        // What each bone is written with: the local its shown pose needs (absolute, the same
        // however often it is written; `ragdoll_core::pose::local_for`).
        // A bone the node tree hangs elsewhere than its physics parent (FNV's spine, under
        // `Bip01 NonAccum` beside the pelvis) is written at its shown joint: it follows the
        // pelvis's move and turn as the body has it.
        let shown_at = shown_positions(&rig, &anim, &shown, pi, pelvis_offset);
        let locals: Vec<(M3, V3)> = (0..n)
            .map(|b| {
                let bone = &rig.bones[b];
                let base = bone.pose.unwrap_or_default();
                if resting {
                    base
                } else {
                    let shift = if bone.role == Role::Pelvis {
                        Some(pelvis_offset)
                    } else if bone.parent.is_none() && bone.phys.is_some() {
                        Some(sub(shown_at[b], anim[b].1))
                    } else {
                        None
                    };
                    pose::local_for(&parents[b], base, &offsets[b], shift)
                }
            })
            .collect();
        for (bone, &(rot, t)) in rig.bones.iter_mut().zip(&locals) {
            if rot.iter().all(|x| x.is_finite()) && t.iter().all(|x| x.is_finite()) {
                write9(e, bone.node + LOCAL_ROTATION, &rot);
                write3(e, bone.node + LOCAL_TRANSLATION, t);
                bone.written = Some((rot, t));
                if let Some(raw) = bone.raw {
                    written_now.push((bone.node, raw));
                }
            }
        }
        update(e, root);
        // The nodes the drawing is checked on (`after_draw`): the pelvis, the spine's top, the
        // head, with their animated world.
        rig.drawn_ref = [Role::Pelvis, Role::Spine, Role::Head]
            .iter()
            .filter_map(|&role| {
                let i = (0..n).rev().find(|&i| rig.bones[i].role == role)?;
                Some((rig.bones[i].node, anim[i].0, anim[i].1))
            })
            .collect();
        rig.pelvis_now = length(pelvis_offset);
        // The shown pelvis's turn against its animation and its move beyond the actor's own, for
        // the clips' samples (a spin, a snap back).
        let shown_pelvis = add(anim[pi].1, pelvis_offset);
        let yaw = active::heading_off(&mul(&shown[pi], &transpose(&anim[pi].0)), forward);
        if let Some((p0, yaw0)) = rig.shown_before {
            rig.yaw_rate_now = active::yaw_between(yaw0, yaw) / dt.max(1e-4);
            rig.jump_now = length(sub(sub(shown_pelvis, p0), moved_by_game));
        }
        rig.shown_before = Some((shown_pelvis, yaw));
        // Each bone's shown offset from its animation, and the fastest it changed (a spin).
        let offsets_now: Vec<active::Q> = shown
            .iter()
            .zip(anim.iter())
            .map(|(s, a)| active::qmul(active::qfrom_m(s), active::qconj(active::qfrom_m(&a.0))))
            .collect();
        rig.spin_now = 0.0;
        if rig.offsets_before.len() == offsets_now.len() {
            for (b, (q, q0)) in offsets_now.iter().zip(&rig.offsets_before).enumerate() {
                let w = length(active::qlog(active::qmul(*q, active::qconj(*q0)))) / dt.max(1e-4);
                if w > rig.spin_now {
                    rig.spin_now = w;
                    rig.spin_bone = rig.bones[b].role;
                }
            }
        }
        rig.offsets_before = offsets_now;
        rig.spread_now = rig.body.as_ref().map_or(0.0, |b| spread_of_body(b, pi));
        // The pose shown, over the animation: where a fade starts if the body is dropped.
        rig.last = (!resting).then(|| {
            let anim_r: Vec<M3> = anim.iter().map(|a| a.0).collect();
            (active::Fade::offsets(&shown, &anim_r), pelvis_offset)
        });
        if resting && rig.since.elapsed().as_secs_f32() > 0.2 {
            note(format!(
                "{}: back in its animation (drawn off it at most {:.1} units, {:.1} degrees)",
                describe(e, rig.actor),
                rig.drawn_most.0,
                rig.drawn_most.1
            ));
            continue;
        }
        keep.push(rig);
    }
    *WRITTEN.lock().unwrap_or_else(|p| p.into_inner()) = written_now;
    let mut rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    // Rigs a hit made while this ran come after.
    let fresh = std::mem::take(&mut *rigs);
    *rigs = keep;
    for f in fresh {
        rigs.retain(|r| r.actor != f.actor);
        rigs.push(f);
    }
}

/// The body's most spread: the largest distance of a bone's centre from the pelvis's (m;
/// `active::Body::check` drops the body past `active::MAX_SPREAD`).
fn spread_of_body(b: &active::Body, pi: usize) -> f32 {
    let pelvis = b.state.get(pi).map_or([0.0; 3], |s| s.com);
    b.state
        .iter()
        .map(|s| length(sub(s.com, pelvis)))
        .fold(0.0f32, f32::max)
}

/// The clips' sample of an actor's reaction (`crate::room_record`): `"body":{...}` with whether
/// its physical body is on, the pelvis offset written (game units), the body's most spread (m),
/// the base drift (reads a later pass moved, the most in tolerances), the animation jumps eased
/// in, and how much of a dropped body's fade is left; `"body":null` when it is not reacting.
pub fn sample_fields(actor: u32) -> String {
    let rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    match rigs.iter().find(|r| r.actor == actor) {
        Some(r) => format!(
            "\"body\":{{\"active\":{},\"pelvis_offset\":{},\"pelvis_jump\":{},\"yaw_rate\":{},\"spin\":{},\"spin_bone\":{},\"settling\":{},\"yielded\":{},\"limited\":{},\"ai_speed\":{},\"drawn_offset\":{},\"drawn_turn\":{},\"drawn_skin\":[{},{},{}],\"spread\":{},\"drift\":{},\"drift_most\":{},\"jumps\":{},\"fade\":{}}}",
            r.body.is_some(),
            room_events::num(r.pelvis_now),
            room_events::num(r.jump_now),
            room_events::num(r.yaw_rate_now),
            room_events::num(r.spin_now),
            room_events::text(
                BONES
                    .iter()
                    .find(|(_, role)| *role == r.spin_bone)
                    .map_or("a bone", |b| b.0)
            ),
            r.settle.holding(),
            r.yield_at.is_some(),
            r.limited,
            room_events::num(r.ai_speed),
            room_events::num(r.drawn_offset),
            room_events::num(r.drawn_turn),
            r.drawn_skin.0,
            r.drawn_skin.1,
            r.drawn_skin.2,
            room_events::num(r.spread_now),
            r.drift,
            room_events::num(r.drift_most),
            r.jumps,
            room_events::num(r.fade.as_ref().map_or(0.0, |f| f.left())),
        ),
        None => "\"body\":null".to_string(),
    }
}

/// Whether an actor is reacting now (a rig the pose layer writes): the clips sample it every
/// frame then.
pub fn reacting(actor: u32) -> bool {
    RIGS.lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|r| r.actor == actor)
}

/// The actors reacting now (a rig the pose layer writes): the regular game's clips sample each
/// every frame (`crate::room_record`).
pub fn reacting_actors() -> Vec<u32> {
    RIGS.lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .map(|r| r.actor)
        .collect()
}

/// Each bone's world offset over its animation: its parent's shown and animated rotations taken
/// back, its own shown one on (`A(p) S(p)^T S(b) A(b)^T`), made orthonormal.
fn offsets_of(bones: &[Bone], anim: &[(M3, V3)], shown: &[M3]) -> Vec<M3> {
    (0..bones.len())
        .map(|b| {
            let own = mul(&shown[b], &transpose(&anim[b].0));
            pose::ortho(&match bones[b].parent {
                Some(p) => mul(&mul(&anim[p].0, &transpose(&shown[p])), &own),
                None => own,
            })
        })
        .collect()
}

/// The animated pose in the world (each bone's) and each bone's parent node's animated world,
/// worked out from the animation's own locals (the bases) down from the actor's root, never read
/// back from the bones, which may hold the last write.
fn animated(e: &Engine, rig: &Rig) -> (Vec<(M3, V3)>, Vec<pose::Xf>) {
    let n = rig.bones.len();
    let mut world: Vec<pose::Xf> = Vec::with_capacity(n);
    let mut parents: Vec<pose::Xf> = Vec::with_capacity(n);
    for bone in &rig.bones {
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
    (world.iter().map(|w| (w.r, w.t)).collect(), parents)
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
        let p = match rig.bones[b].phys {
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
/// spine's, the neck's), its mass its share of the body's. In metres from the rig's origin
/// ([`Rig::origin`]).
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
            let children: Vec<usize> = (0..n).filter(|&c| rig.bones[c].phys == Some(b)).collect();
            let next = children
                .iter()
                .copied()
                .find(|&c| matches!(rig.bones[c].role, Role::Spine | Role::Neck | Role::Head))
                .or_else(|| children.first().copied());
            let end = match next {
                Some(c) => now[c].1,
                None => {
                    let from = bone
                        .phys
                        .map_or(add(now[b].1, [0.0, 0.0, -1.0]), |p| now[p].1);
                    let dir = unit(sub(now[b].1, from)).unwrap_or([0.0, 0.0, 1.0]);
                    add(
                        now[b].1,
                        scale(dir, leaf_length(bone.role) * scale_ * UNITS_PER_METRE),
                    )
                }
            };
            active::Bone {
                parent: bone.phys,
                role: bone.role,
                q: active::qfrom_m(&now[b].0),
                joint: scale(sub(now[b].1, rig.origin), m),
                end: scale(sub(end, rig.origin), m),
                radius: radius(bone.role) * scale_,
                mass: rig.mass * shares[b] / total,
                before: Some((
                    active::qfrom_m(&before[b].0),
                    scale(sub(before[b].1, rig.origin), m),
                )),
            }
        })
        .collect();
    active::Body::from_pose(&bones, dt)
}

/// The physical body's bones' velocities (game units a second) by node, for the ragdoll to go on
/// with when it takes the body (`crate::knockdown`): nothing jumps at the switch. Held
/// (`active::handover_velocities`): the pelvis's to the hit's speed (or the fall's), each bone's
/// own motion about it to the spin bound.
pub fn body_velocities(actor: u32) -> Vec<(u32, V3)> {
    let rigs = RIGS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(rig) = rigs.iter().find(|r| r.actor == actor) else {
        return Vec::new();
    };
    let Some(body) = &rig.body else {
        return Vec::new();
    };
    let Some(pi) = bone_of(rig, Role::Pelvis, 2) else {
        return Vec::new();
    };
    let most = rig.react_v.max(rig.fall.as_ref().map_or(0.0, |f| f.speed));
    let held = active::handover_velocities(&body.state, pi, most);
    rig.bones
        .iter()
        .zip(&held)
        .map(|(b, v)| (b.node, scale(*v, UNITS_PER_METRE)))
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
///
/// It reads the animated pose `anim` (worked out with the root's world at `root_at`), never the
/// shown one (which holds the last write: the steps chased their own output). A step's move of
/// the reference is put in [`Rig::placed`] until the root's world shows it. When the steps are
/// done their pelvis offset is eased out ([`Rig::shift_back`]), not dropped.
fn balance_frame(
    e: &mut Engine,
    rig: &mut Rig,
    anim: &[(M3, V3)],
    root_at: V3,
    dt: f32,
) -> Vec<Option<M3>> {
    use crate::balance::Outcome;
    let mut out: Vec<Option<M3>> = vec![None; rig.bones.len()];
    let Some(mut s) = rig.balance.take() else {
        return out;
    };
    rig.shift_back = None;
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
        rig.own_moved = add(rig.own_moved, moved);
        rig.own_total = add(rig.own_total, moved);
        // Any earlier move the root's world has not shown yet goes with this one.
        let waiting = rig
            .placed
            .map_or([0.0; 3], |(was, m, _)| sub(m, sub(root_at, was)));
        rig.placed = Some((root_at, add(waiting, moved), Instant::now()));
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
    // The animated pose at the reference as moved (the root's world may not show it yet).
    let ahead = rig
        .placed
        .map_or([0.0; 3], |(was, m, _)| sub(m, sub(root_at, was)));
    let at = |i: usize| add(anim[i].1, ahead);
    let pelvis = bone_of(rig, Role::Pelvis, 2).map(at);
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
        let (hip0, knee0, ankle0) = (at(t), at(c), at(f));
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
            "{}: steps done ({:?}): back to its animation (the pelvis's {:.0} units eased out)",
            describe(e, actor),
            s.outcome,
            length(shift)
        ));
        rig.shift_back = Some((shift, Instant::now(), SHIFT_BACK));
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
    // The AI walking or turning it: its stride carries it (steps planned from a stride and
    // followed while the AI walks it threw the pelvis sideways).
    if active::ai_walking(rig.ai_speed, rig.ai_turn) {
        let (speed, turn) = (rig.ai_speed, rig.ai_turn);
        drop(rigs);
        note(format!(
            "{}: no catch steps: its AI is walking it ({speed:.0} units/s, {turn:.2} rad/s)",
            describe(e, actor)
        ));
        return;
    }
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
            facing: facing(e, rig),
            walked: 0.0,
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
                // The bone whose segment the impact is on (as the flinch names it).
                let joints: Vec<V3> = rig
                    .bones
                    .iter()
                    .map(|b| read3(e, b.node + WORLD_TRANSLATION))
                    .collect();
                let parents: Vec<Option<usize>> = rig.bones.iter().map(|b| b.parent).collect();
                let b = rig
                    .bones
                    .get(ragdoll_core::flinch::struck_bone(&joints, &parents, point))?;
                Some(condition::region_of(b.role, b.side))
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
                    surface.is_none() || hands::reaches(at(u), target, reach, facing(e, rig), side);
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
            facing: facing(e, rig),
            walked: 0.0,
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

/// The game's own move of the actor since the frame before (its AI walking it: the reference's
/// move less the steps' own, [`Rig::own_moved`]), carried by the reaction: the feet where they
/// stand and step, a brace's count of it (its hands let go once the actor is walked away); the
/// physical body rides on the animation itself (`active::Body::carry`). How fast the AI moves and
/// turns it, and past `active::yields` since the hit, the reaction yields to its animation. Without it the body trailed the walking actor on its pelvis spring and
/// snapped back to it as it was handed back (Patrick's c2a53c0 fold clip: 18 to 1.3 units in 0.25
/// s). Not a body going over (that is physical). Answers the move.
fn carry(e: &Engine, rig: &mut Rig, dt: f32) -> V3 {
    let reference = read3(e, rig.actor + REFR_POSITION);
    let own = std::mem::take(&mut rig.own_moved);
    let moved = rig
        .carried
        .map_or([0.0; 3], |last| sub(sub(reference, last), own));
    rig.carried = Some(reference);
    // How fast the AI moves and turns it (eased over a few frames), and how far since the hit:
    // past `active::yields` the reaction yields to the animation.
    let now = facing(e, rig);
    let turned = rig.facing_before.map_or(0.0, |f| hands::turned(f, now));
    rig.facing_before = Some(now);
    if dt > 0.0 {
        let k = (dt / 0.15).min(1.0);
        rig.ai_speed += (length([moved[0], moved[1], 0.0]) / dt - rig.ai_speed) * k;
        rig.ai_turn += (turned / dt - rig.ai_turn) * k;
    }
    if rig.body.is_none() || rig.fall.is_some() {
        return moved;
    }
    rig.moved_since_hit += length([moved[0], moved[1], 0.0]);
    rig.turned_since_hit += turned;
    if active::yields(rig.moved_since_hit, rig.turned_since_hit) {
        yield_now(rig, "its AI moves it");
    }
    if length(moved) <= 1e-4 {
        return moved;
    }
    // The body rides on the animation's own motion (`Body::carry`, in the frame's step); the
    // feet where they stand and step, and a brace's count, go with the actor here.
    if let Some((feet, _)) = rig.planted.as_mut() {
        for f in feet.iter_mut() {
            *f = add(*f, moved);
        }
    }
    if let Some(b) = rig.brace.as_mut() {
        b.walked += length(moved);
    }
    if let Some(s) = rig.balance.as_mut() {
        s.carry(moved);
    }
    moved
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
    let forward = facing(e, rig);
    // Yielded to its animation: the hands let go.
    let let_go = if rig.yield_at.is_some() {
        Some("its animation moved on")
    } else {
        hands::let_go(
            rig.balance.is_some(),
            b.t,
            com,
            rig.hit_at.elapsed().as_secs_f32(),
            hands::turned(b.facing, forward),
            b.walked,
        )
    };
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
        // Inside the shoulder's human range (never up behind the back).
        let target = hands::held_goal(
            anim[u].1,
            brace_target(anim[u].1, b.dir, b.wall, b.top),
            forward,
            side,
        );
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
    let facing_now = facing(e, rig);
    // It goes over physically (the support fades over the lead, [`apply`]); a windmill is the
    // one pose it is given.
    for side in 0..2u8 {
        if f.windmill && rig.limbs.arm(side as usize) {
            if let Some(u) = bone_of(rig, Role::UpperArm, side) {
                let phase = if side == 0 { 0.0 } else { std::f32::consts::PI };
                let a = WINDMILL_SWING * (std::f32::consts::TAU * WINDMILL_HZ * f.t + phase).sin();
                // Inside the shoulder's human range (never up behind the back).
                let dir = bone_of(rig, Role::Forearm, side).map_or([0.0, 0.0, -1.0], |fo| {
                    sub(
                        read3(e, rig.bones[fo].node + WORLD_TRANSLATION),
                        read3(e, rig.bones[u].node + WORLD_TRANSLATION),
                    )
                });
                out[u] = Some(ragdoll_core::flinch::held_turn(
                    &rotation(scale(across, a)),
                    dir,
                    facing_now,
                    side,
                ));
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
             was asked (knock state 0); eased back up over {FALL_BACK} s",
            describe(e, rig.actor)
        ));
        stand_back_up(rig, &f);
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
        if let Some(f) = r.fall.take() {
            stand_back_up(r, &f);
            note(format!(
                "{actor:08X}: the knock-down did not happen: the fall pose eases back up over \
                 {FALL_BACK} s"
            ));
        }
    }
}

/// A fall pose that is not going into the ragdoll after all (a refused or given-up knock-down):
/// its support and its pelvis offset eased back over [`FALL_BACK`] (they were let go in one frame:
/// the body snapped upright, Patrick's 10:47 clip on 454ebb2).
fn stand_back_up(rig: &mut Rig, f: &Fall) {
    rig.support_back = Some((fall_support(f), Instant::now()));
    rig.shift_back = Some((rig.shift, Instant::now(), FALL_BACK));
}

/// The support's carrying of a body going over: eased down over its lead, never under
/// [`FALL_SUPPORT`] (the ragdoll takes it; a posed body left with none folded into what was behind
/// it and jittered).
fn fall_support(f: &Fall) -> f32 {
    (1.0 - active::ease(f.t / f.lead.max(1e-3))).max(FALL_SUPPORT)
}

/// The least of the support while the posed body goes over, and how long a fall pose that did
/// not go into the ragdoll takes to ease back up (s).
const FALL_SUPPORT: f32 = 0.5;
const FALL_BACK: f32 = 0.6;

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
    // A fall pose only for a knock-down that will happen (it went over, then stood up again).
    if let Err(why) = crate::knockdown::can_knock_now(e, actor) {
        note(format!("{}: no fall pose: {why}", describe(e, actor)));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests of the write's put-back share [`WRITTEN`]: one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// After the draw every bone written gets back the game's own local (what it held when read),
    /// so the game's next frame never starts from the reaction's write (Patrick's 14-16-02 clip:
    /// a write left in the bones was read back stale, then taken for the animation).
    #[test]
    fn the_write_is_put_back_after_the_draw() {
        let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let mut e = Engine::new();
        e.mem.map(0x4000, 0x1000);
        let node = 0x4000u32;
        let game: (M3, V3) = (
            ragdoll_core::flinch::rotation([0.0, 0.0, 0.3]),
            [1.0, 2.0, 3.0],
        );
        let ours: (M3, V3) = (
            ragdoll_core::flinch::rotation([0.4, 0.0, 0.0]),
            [5.0, 6.0, 7.0],
        );
        write9(&mut e, node + LOCAL_ROTATION, &ours.0);
        write3(&mut e, node + LOCAL_TRANSLATION, ours.1);
        *WRITTEN.lock().unwrap() = vec![(node, game)];
        unwrite(&mut e);
        assert_eq!(read9(&e, node + LOCAL_ROTATION), game.0);
        assert_eq!(read3(&e, node + LOCAL_TRANSLATION), game.1);
        assert!(WRITTEN.lock().unwrap().is_empty());
    }

    /// The call order of a frame with the reactions in it (`around_draw`): the write first, then
    /// the world pass (`00873200`, where the actors are culled, skinned and drawn) sees it and
    /// reads what it draws with, then the game's own locals put back after the drawing (00262dd
    /// wrote after the world pass and put back after the frame: never drawn).
    #[test]
    fn the_world_pass_draws_the_write_then_it_is_put_back() {
        let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let mut e = Engine::new();
        e.mem.map(0x5000, 0x1000);
        let node = 0x5000u32;
        let game: (M3, V3) = (
            ragdoll_core::flinch::rotation([0.0, 0.0, 0.3]),
            [1.0, 2.0, 3.0],
        );
        let ours: (M3, V3) = (
            ragdoll_core::flinch::rotation([0.4, 0.0, 0.0]),
            [5.0, 6.0, 7.0],
        );
        let order = std::sync::Arc::new(Mutex::new(Vec::<&str>::new()));
        let seen = std::sync::Arc::new(Mutex::new(None));
        let (o, sn) = (order.clone(), seen.clone());
        e.register_double(WORLD_PASS, move |e, _| {
            o.lock().unwrap().push("world pass");
            *sn.lock().unwrap() = Some((
                read3(e, node + LOCAL_TRANSLATION),
                DRAWN_PENDING.load(std::sync::atomic::Ordering::Relaxed),
            ));
            0u32.into_ret()
        });
        let o = order.clone();
        around_draw_with(
            &mut e,
            |e| {
                o.lock().unwrap().push("write");
                write9(e, node + LOCAL_ROTATION, &ours.0);
                write3(e, node + LOCAL_TRANSLATION, ours.1);
                *WRITTEN.lock().unwrap() = vec![(node, game)];
            },
            |e| {
                e.call(WORLD_PASS, &args![0u32, 0u32, 1u32, 1u32, 0u32]);
            },
        );
        assert_eq!(*order.lock().unwrap(), vec!["write", "world pass"]);
        assert_eq!(
            *seen.lock().unwrap(),
            Some((ours.1, true)),
            "the world pass drew the write, its drawn pose still to read"
        );
        assert_eq!(
            read3(&e, node + LOCAL_TRANSLATION),
            game.1,
            "put back after"
        );
        assert!(!DRAWN_PENDING.load(std::sync::atomic::Ordering::Relaxed));
    }

    /// What is drawn is read from the bones the skinned geometry lists, by name: a scene of a root
    /// node with a skinned geometry whose skin lists a pelvis bone (a node of its own, not the
    /// rig's) 6 units over its animated place and turned 0.2 rad.
    #[test]
    fn the_drawn_pose_is_read_from_the_skins_bones() {
        let mut e = Engine::new();
        e.mem.map(0x6000, 0x4000);
        let (root, geometry, skin, data, array) =
            (0x6000u32, 0x6200u32, 0x6400u32, 0x6500u32, 0x6600u32);
        let (bone, rig_node, name) = (0x6800u32, 0x6a00u32, 0x6c00u32);
        let (node_vt, geo_vt, leaf_vt) = (0x6e00u32, 0x6f00u32, 0x6f80u32);
        let (is_self, is_none) = (0x7000_0100u32, 0x7000_0104u32);
        e.register_double(is_self, |_, a| a[0].into_ret());
        e.register_double(is_none, |_, _| 0u32.into_ret());
        e.mem.set_u32(node_vt + IS_NODE_SLOT, is_self);
        e.mem.set_u32(geo_vt + IS_NODE_SLOT, is_none);
        e.mem.set_u32(geo_vt + IS_GEOMETRY_SLOT, is_self);
        e.mem.set_u32(leaf_vt + IS_NODE_SLOT, is_none);
        e.mem.set_u32(leaf_vt + IS_GEOMETRY_SLOT, is_none);
        e.mem.set_u32(root, node_vt);
        e.mem.set_u32(geometry, geo_vt);
        e.register_double(CHILD_COUNT, move |_, a| u32::from(a[0] == root).into_ret());
        e.register_double(CHILD_AT, move |_, _| geometry.into_ret());
        e.mem.set_u32(geometry + GEOMETRY_SKIN, skin);
        e.mem.set_u32(skin + SKIN_DATA, data);
        e.mem.set_u32(skin + SKIN_BONES, array);
        e.mem.set_u32(data + SKIN_DATA_BONES, 1);
        e.mem.set_u32(array, bone);
        e.mem.set_cstr(name, b"Bip01 Pelvis");
        e.mem.set_u32(bone + NODE_NAME, name);
        e.mem.set_u32(rig_node + NODE_NAME, name);
        let anim_r = ragdoll_core::flinch::IDENTITY;
        let anim_p: V3 = [10.0, 20.0, 90.0];
        write9(
            &mut e,
            bone + WORLD_ROTATION,
            &ragdoll_core::flinch::rotation([0.2, 0.0, 0.0]),
        );
        write3(&mut e, bone + WORLD_TRANSLATION, [10.0, 26.0, 90.0]);
        let bones = skin_bones(&mut e, root);
        assert_eq!(bones, vec![bone]);
        let (most, turn, matched, same) =
            drawn_against(&mut e, &bones, &[(rig_node, anim_r, anim_p)]);
        assert!((most - 6.0).abs() < 1e-3, "{most}");
        assert!((turn - 0.2f32.to_degrees()).abs() < 1e-2, "{turn}");
        assert_eq!((matched, same), (1, 0));
    }

    /// The write is in the bones through the whole frame's drawing (the world is drawn after the
    /// first-person set-up where it is written), and put back only after it, also when the
    /// drawing unwinds (Patrick's d0f4192 clips: put back before the world was drawn, no reaction
    /// was ever seen).
    #[test]
    fn the_write_is_drawn_then_put_back() {
        let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let mut e = Engine::new();
        e.mem.map(0x5000, 0x1000);
        let node = 0x5000u32;
        let game: (M3, V3) = (
            ragdoll_core::flinch::rotation([0.0, 0.0, 0.3]),
            [1.0, 2.0, 3.0],
        );
        let ours: (M3, V3) = (
            ragdoll_core::flinch::rotation([0.4, 0.0, 0.0]),
            [5.0, 6.0, 7.0],
        );
        let write = |e: &mut Engine| {
            write9(e, node + LOCAL_ROTATION, &ours.0);
            write3(e, node + LOCAL_TRANSLATION, ours.1);
            *WRITTEN.lock().unwrap() = vec![(node, game)];
        };
        write(&mut e);
        let mut seen = None;
        draw_then_put_back(&mut e, |e| {
            seen = Some(read3(e, node + LOCAL_TRANSLATION));
        });
        assert_eq!(seen, Some(ours.1), "drawn with the reaction's write");
        assert_eq!(
            read3(&e, node + LOCAL_TRANSLATION),
            game.1,
            "put back after"
        );
        // An unwinding draw still puts it back.
        write(&mut e);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            draw_then_put_back(&mut e, |_| panic!("the drawing stops"));
        }));
        assert!(unwound.is_err());
        assert_eq!(read3(&e, node + LOCAL_TRANSLATION), game.1);
    }
}
