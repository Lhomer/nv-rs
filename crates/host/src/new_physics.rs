//! New Physics in the game (`[nv-rs Gunplay] bNewPhysics`, off by default; docs/GUNPLAY.md "New
//! Physics"): a hit on a human whose 3D has FNV's 18 ragdoll bodies' nodes hands it to one
//! balance controller (`ragdoll_core::np`, the code the ragdoll lab runs) in place of the hit
//! reactions, catch steps and knockdowns of `crate::active_ragdoll` and `crate::knockdown`, which
//! stay as they are for everyone else (the switch off, a creature, a body it cannot take).
//!
//! Each frame, at the draw's start (`crate::active_ragdoll::apply`, the same write point): the
//! animated pose read (each body's node's world, as the game's update left it), the controller
//! stepped on it, and its bodies written as the nodes' locals (each solved from its parent's world
//! as written, the nodes between the bodies kept as animated) and the tree updated, so the world
//! pass draws the simulated body; then read back (the drawn against the simulated, said if it ever
//! differs); after the draw the game's own locals put back (`crate::active_ragdoll::unwrite`). The
//! body's move against its animation after a step is the actor's: its reference moved by it.
//!
//! While it has an actor the game's own reaction to its hits is not played: the physical hit
//! reaction (`Actor::HitMe_ov2`, `008a4af0`), the crippled-limb idle (the setting byte `011df760`
//! cleared around `Actor::HitMe`), `KnockExplosion` from `HitMe` (`crate::hit_force`'s wrapper),
//! and the controller's stored push.

use crate::room_events;
use engine::prelude::*;
use ragdoll_core::flinch::{add, apply_m, length, mul, scale, sub, transpose, M3, V3};
use ragdoll_core::np::Np;
use ragdoll_core::np_balance::Phase;
use ragdoll_core::np_body::Model;
use ragdoll_core::pose::{self, Xf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const ACTOR_3D: u32 = 0x0043_fcd0;
const LOOKUP_FORM: u32 = 0x0048_39c0;
const FORM_ID: u32 = 0x0c;
const PROCESS: u32 = 0x68;
const PROCESS_LEVEL: u32 = 0x28;
const MIDDLE_HIGH: u8 = 1;
const KNOCK_STATE: u32 = 0x13c;
const LIFE_STATE: u32 = 0x108;
const REFR_POSITION: u32 = 0x30;
const PARENT: u32 = 0x18;
const LOCAL_ROTATION: u32 = 0x34;
const LOCAL_TRANSLATION: u32 = 0x58;
const LOCAL_SCALE: u32 = 0x64;
const WORLD_ROTATION: u32 = 0x68;
const WORLD_TRANSLATION: u32 = 0x8c;
const WORLD_SCALE: u32 = 0x98;
const PLAYER: u32 = 0x011d_ea3c;
/// `Actor::HitMe_ov2` (the physical hit reaction) and the crippled-limb idle's setting byte.
pub const HIT_ME_OV2: u32 = 0x008a_4af0;
pub const CRIPPLE_IDLE: u32 = 0x011d_f760;

/// How long after its last hit, standing calm, an actor is handed back to its animation (s), and
/// how far its drawn body may be off the animation then (game units).
pub const LET_GO_AFTER: f32 = 1.5;
pub const LET_GO_OFF: f32 = 1.5;
/// The longest frame stepped (s).
const MAX_DT: f32 = 0.05;

/// FNV's ragdoll's model, made once.
fn model() -> &'static Model {
    static M: OnceLock<Model> = OnceLock::new();
    M.get_or_init(Model::fnv)
}

/// One actor New Physics has.
struct NpActor {
    actor: u32,
    id: u32,
    root: u32,
    /// Each body's node.
    nodes: Vec<u32>,
    /// Each body's nearest body above it (its index) and the nodes between (top down, its
    /// parent last); none above: its node's parent's world as the game has it.
    chains: Vec<(Option<usize>, Vec<u32>)>,
    np: Option<Np>,
    /// Hits waiting for its first frame (body, point in game units, impulse N·s), and the pose it
    /// was taken in (for its animation's velocity then).
    pending: Vec<(usize, V3, V3)>,
    taken_pose: Vec<(M3, V3)>,
    /// Seconds since its last hit.
    since_hit: f32,
    /// The last frame's drawn bodies (rotation, game units) and the read-back's most (degrees,
    /// units), and the reaction's sums.
    drawn: Vec<(M3, V3)>,
    readback: (f32, f32),
    readback_most: (f32, f32),
    spine_peak: f32,
    steps: u32,
    moved: V3,
    frames: u32,
}

static ACTORS: Mutex<Vec<NpActor>> = Mutex::new(Vec::new());
static WRITTEN: Mutex<Vec<(u32, (M3, V3))>> = Mutex::new(Vec::new());
static ROOTS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static LAST: Mutex<Option<Instant>> = Mutex::new(None);
/// The falls handed over (actor, the root body's speed and the body's centre's, game units a
/// second): the tests' and the log's.
static FALLS: Mutex<Vec<Fall>> = Mutex::new(Vec::new());

/// A fall handed over: the actor, its root body's speed and its centre's (game units a second),
/// and the pose handed (each body's node and its world position, game units).
#[derive(Clone, Debug)]
pub struct Fall {
    pub actor: u32,
    pub root: f32,
    pub centre: f32,
    pub pose: Vec<(u32, V3)>,
}

/// The falls handed over since the last [`clear`].
pub fn falls() -> Vec<Fall> {
    FALLS.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

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

fn read3(e: &Engine, at: u32) -> V3 {
    [0, 4, 8].map(|o| e.mem.f32(at + o))
}
fn read9(e: &Engine, at: u32) -> M3 {
    std::array::from_fn(|i| e.mem.f32(at + 4 * i as u32))
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
fn read_world(e: &Engine, node: u32) -> Xf {
    Xf {
        r: read9(e, node + WORLD_ROTATION),
        t: read3(e, node + WORLD_TRANSLATION),
        s: e.mem.f32(node + WORLD_SCALE),
    }
}
fn read_local(e: &Engine, node: u32) -> Xf {
    Xf {
        r: read9(e, node + LOCAL_ROTATION),
        t: read3(e, node + LOCAL_TRANSLATION),
        s: e.mem.f32(node + LOCAL_SCALE),
    }
}

/// Whether New Physics has `actor` now.
pub fn owns(actor: u32) -> bool {
    actor != 0
        && ACTORS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .any(|a| a.actor == actor)
}

/// The actors New Physics has now.
pub fn actors() -> Vec<u32> {
    ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .map(|a| a.actor)
        .collect()
}

/// Whether New Physics is on (the switch and the weighty combat row).
pub fn on() -> bool {
    crate::gunplay::is_on() && crate::gunplay::tuning().new_physics
}

/// Takes `actor` for a hit if it can (before `Actor::HitMe` runs, so the game's own reaction is
/// not played): New Physics on; a living, middle-high or higher character other than the player,
/// standing (knock state 0); its 3D with FNV's 18 bodies' nodes. False leaves it to the current
/// reactions.
pub fn claim(e: &mut Engine, actor: u32) -> bool {
    if !on() || actor == 0 {
        return false;
    }
    if owns(actor) {
        return true;
    }
    if actor == e.global::<u32>(PLAYER) || !e.vcall(actor, 0x100, &args![]).bool() {
        return false;
    }
    let process = e.mem.u32(actor + PROCESS);
    if process == 0
        || e.mem.u8(process + PROCESS_LEVEL) > MIDDLE_HIGH
        || e.mem.u8(process + KNOCK_STATE) != 0
        || matches!(e.mem.u32(actor + LIFE_STATE), 1 | 2)
    {
        return false;
    }
    let root = e.call(ACTOR_3D, &args![actor]).u32();
    if root == 0 {
        return false;
    }
    let m = model();
    let mut nodes = Vec::with_capacity(m.bodies.len());
    for b in &m.bodies {
        match crate::knockdown::named_node(e, actor, b.node.as_bytes()) {
            Some(n) => nodes.push(n),
            None => return false,
        }
    }
    // Each body's chain up to the nearest body above it.
    let chains = nodes
        .iter()
        .map(|&n| {
            let mut between = Vec::new();
            let mut at = e.mem.u32(n + PARENT);
            let mut depth = 0;
            while at != 0 && depth < 16 {
                if let Some(i) = nodes.iter().position(|&x| x == at) {
                    between.reverse();
                    return (Some(i), between);
                }
                between.push(at);
                at = e.mem.u32(at + PARENT);
                depth += 1;
            }
            (None, Vec::new())
        })
        .collect();
    let pose: Vec<(M3, V3)> = nodes
        .iter()
        .map(|&n| {
            (
                read9(e, n + WORLD_ROTATION),
                read3(e, n + WORLD_TRANSLATION),
            )
        })
        .collect();
    let id = e.mem.u32(actor + FORM_ID);
    note(format!(
        "{id:08X}: [np take] New Physics has it (its 18 bodies' nodes found): the current reactions \
         and its own hit reaction are off for it"
    ));
    ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(NpActor {
            actor,
            id,
            root,
            nodes,
            chains,
            np: None,
            pending: Vec::new(),
            taken_pose: pose,
            since_hit: 0.0,
            drawn: Vec::new(),
            readback: (0.0, 0.0),
            readback_most: (0.0, 0.0),
            spine_peak: 0.0,
            steps: 0,
            moved: [0.0; 3],
            frames: 0,
        });
    true
}

/// A hit on `actor` (it must be [`claim`]ed): `p` its momentum (N·s, `crate::knockdown`'s
/// measure, Hit Force applied), at `point` (world, game units) along `direction` (unit). The
/// struck body is the nearest's to the point.
pub fn hit(e: &mut Engine, actor: u32, p: f32, point: V3, direction: V3) {
    let mut actors = ACTORS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(a) = actors.iter_mut().find(|a| a.actor == actor) else {
        return;
    };
    let m = model();
    let worlds: Vec<(M3, V3)> = a
        .nodes
        .iter()
        .map(|&n| {
            (
                read9(e, n + WORLD_ROTATION),
                read3(e, n + WORLD_TRANSLATION),
            )
        })
        .collect();
    let body = nearest_body(m, &worlds, point);
    let impulse = scale(direction, p);
    a.since_hit = 0.0;
    note(format!(
        "{:08X}: [np hit] {} N s on {} ({:.2} m/s on its {:.0} kg)",
        a.id,
        format_args!("{p:.1}"),
        m.bodies[body].node,
        p / m.mass,
        m.mass
    ));
    match a.np.as_mut() {
        Some(np) => np.hit(body, point, impulse),
        None => a.pending.push((body, point, impulse)),
    }
}

/// The body nearest `point` (its capsule's segment, each body at its node's world, game units).
fn nearest_body(m: &Model, worlds: &[(M3, V3)], point: V3) -> usize {
    let k = ragdoll_core::np_body::UNITS;
    let mut best = (f32::INFINITY, 0);
    for (i, (b, (r, t))) in m.bodies.iter().zip(worlds).enumerate() {
        let ends = b.ends.map(|e| add(*t, apply_m(r, scale(e, k))));
        let d = sub(ends[1], ends[0]);
        let l2 = ragdoll_core::flinch::dot(d, d).max(1e-6);
        let s = (ragdoll_core::flinch::dot(sub(point, ends[0]), d) / l2).clamp(0.0, 1.0);
        let dist = length(sub(point, add(ends[0], scale(d, s)))) - b.radius * k;
        if dist < best.0 {
            best = (dist, i);
        }
    }
    best.1
}

/// Once a frame at the draw's start (`crate::active_ragdoll::apply`): each actor's body stepped on
/// its animation and written (module documentation).
pub fn apply(e: &mut Engine) {
    let dt = {
        let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
        let dt = last.map_or(0.0, |t| t.elapsed().as_secs_f32());
        *last = Some(Instant::now());
        dt.clamp(0.0, MAX_DT)
    };
    let list: Vec<NpActor> = std::mem::take(&mut *ACTORS.lock().unwrap_or_else(|p| p.into_inner()));
    if list.is_empty() {
        return;
    }
    let mut keep = Vec::new();
    let mut written = Vec::new();
    let mut roots = Vec::new();
    for mut a in list {
        if !(a.id != 0 && e.call(LOOKUP_FORM, &args![a.id]).u32() == a.actor) {
            continue;
        }
        if e.call(ACTOR_3D, &args![a.actor]).u32() != a.root {
            continue;
        }
        let process = e.mem.u32(a.actor + PROCESS);
        let life = e.mem.u32(a.actor + LIFE_STATE);
        if process == 0 || e.mem.u8(process + KNOCK_STATE) != 0 || matches!(life, 1 | 2) {
            note(format!(
                "{:08X}: [np let go] {} (the game's own ragdoll has it)",
                a.id,
                if matches!(life, 1 | 2) {
                    "dead"
                } else {
                    "knocked down"
                }
            ));
            continue;
        }
        let anim: Vec<(M3, V3)> = a
            .nodes
            .iter()
            .map(|&n| {
                (
                    read9(e, n + WORLD_ROTATION),
                    read3(e, n + WORLD_TRANSLATION),
                )
            })
            .collect();
        let floor = read3(e, a.actor + REFR_POSITION)[2];
        if a.np.is_none() {
            let mut np = Np::new(model().clone(), &a.taken_pose, 1.0);
            if dt > 0.0 {
                let r = model().root;
                np.set_anim_velocity(scale(sub(anim[r].1, a.taken_pose[r].1), 1.0 / dt));
            }
            for (b, p, j) in a.pending.drain(..) {
                np.hit(b, p, j);
            }
            a.np = Some(np);
        }
        let Some(np) = a.np.as_mut() else { continue };
        let drawn = np.frame(&anim, floor, dt);
        a.since_hit += dt;
        a.frames += 1;
        // The bodies written as the nodes' locals, parents first.
        let mut ok = true;
        for (b, &node) in a.nodes.iter().enumerate() {
            let (anc, between) = &a.chains[b];
            let mut up = match anc {
                Some(i) => Xf {
                    r: drawn[*i].0,
                    t: drawn[*i].1,
                    s: e.mem.f32(a.nodes[*i] + WORLD_SCALE),
                },
                None => read_world(e, e.mem.u32(node + PARENT)),
            };
            for &n in between {
                up = up.then(&read_local(e, n));
            }
            let (r, t) = drawn[b];
            let lr = pose::ortho(&mul(&transpose(&up.r), &r));
            let lt = scale(
                apply_m(&transpose(&up.r), sub(t, up.t)),
                1.0 / up.s.max(1e-3),
            );
            if !(lr.iter().all(|x| x.is_finite()) && lt.iter().all(|x| x.is_finite())) {
                ok = false;
                break;
            }
            written.push((
                node,
                (
                    read9(e, node + LOCAL_ROTATION),
                    read3(e, node + LOCAL_TRANSLATION),
                ),
            ));
            write9(e, node + LOCAL_ROTATION, &lr);
            write3(e, node + LOCAL_TRANSLATION, lt);
        }
        if !ok {
            note(format!("{:08X}: [np let go] its pose went bad", a.id));
            continue;
        }
        crate::active_ragdoll::update_tree(e, a.root);
        roots.push(a.root);
        // Read back: what the update left in the nodes against the simulated bodies.
        let mut most = (0.0f32, 0.0f32);
        for (b, &node) in a.nodes.iter().enumerate() {
            let w = read_world(e, node);
            most.0 = most
                .0
                .max(ragdoll_core::np_skeleton::angle_between(&w.r, &drawn[b].0).to_degrees());
            most.1 = most.1.max(length(sub(w.t, drawn[b].1)));
        }
        a.readback = most;
        a.readback_most = (a.readback_most.0.max(most.0), a.readback_most.1.max(most.1));
        let spine = model().body("Bip01 Spine2").unwrap_or(0);
        a.spine_peak = a.spine_peak.max(
            ragdoll_core::np_skeleton::angle_between(&drawn[spine].0, &anim[spine].0).to_degrees(),
        );
        a.drawn = drawn;
        a.steps = np.balance.taken.iter().filter(|s| !s.2).count() as u32;
        // The steps' move is the actor's.
        let mv = np.take_move();
        if length(mv) > 1e-3 {
            let at = read3(e, a.actor + REFR_POSITION);
            crate::knockdown::place(e, a.actor, add(at, mv));
            a.moved = add(a.moved, mv);
        }
        if np.phase() == Phase::Falling {
            // Out of steps (or past any step's reach): the game's ragdoll takes it, moving as its
            // bodies were (`crate::knockdown::hand_over`; the get-up is the knockdowns' own).
            let k = ragdoll_core::np_body::UNITS;
            let velocities: Vec<(u32, V3, V3)> = a
                .nodes
                .iter()
                .zip(&np.states)
                .map(|(&n, s)| (n, scale(s.v, k), s.w))
                .collect();
            let r = model().root;
            let pelvis = length(scale(np.states[r].v, k));
            let (_, com_v) = np.model.centre_of_mass(&np.states);
            let shown: Vec<(u32, M3, V3)> = a
                .nodes
                .iter()
                .zip(&a.drawn)
                .map(|(&n, d)| (n, d.0, d.1))
                .collect();
            let animated: Vec<(u32, M3, V3)> = a
                .nodes
                .iter()
                .zip(&anim)
                .map(|(&n, d)| (n, d.0, d.1))
                .collect();
            match crate::knockdown::hand_over(e, a.actor, &shown, &animated, &velocities) {
                Ok(n) => note(format!(
                    "{:08X}: [np fall] handed to its ragdoll: {n} bodies moving on; the root body \
                     at {pelvis:.0} units/s, the body's centre {:.0} units/s",
                    a.id,
                    length(com_v) * k
                )),
                Err(why) => note(format!(
                    "{:08X}: [np fall] not handed over ({why}): let go",
                    a.id
                )),
            }
            FALLS.lock().unwrap_or_else(|p| p.into_inner()).push(Fall {
                actor: a.actor,
                root: pelvis,
                centre: length(com_v) * k,
                pose: shown.iter().map(|x| (x.0, x.2)).collect(),
            });
            summary(&a);
            continue;
        }
        let calm = np.phase() == Phase::Standing
            && !np.muscles.soft()
            && np.balance.step.is_none()
            && a.since_hit > LET_GO_AFTER;
        if calm {
            let off = a
                .drawn
                .iter()
                .zip(&anim)
                .map(|(d, an)| length(sub(d.1, an.1)))
                .fold(0.0f32, f32::max);
            if off < LET_GO_OFF {
                summary(&a);
                continue;
            }
        }
        keep.push(a);
    }
    WRITTEN
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .extend(written);
    ROOTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .extend(roots);
    let mut actors = ACTORS.lock().unwrap_or_else(|p| p.into_inner());
    // Claimed during the frame (a hit while this ran): kept too.
    keep.append(&mut actors);
    *actors = keep;
}

/// A reaction's summary when New Physics lets go of an actor.
fn summary(a: &NpActor) {
    note(format!(
        "{:08X}: [np reaction] {} frames: spine top at most {:.1} degrees off its animation, {} \
         catching steps, the actor moved {:.0} units; drawn against simulated at most {:.2} degrees \
         and {:.2} units (want 0.5 and 0.5)",
        a.id,
        a.frames,
        a.spine_peak,
        a.steps,
        length(a.moved),
        a.readback_most.0,
        a.readback_most.1
    ));
}

/// After the draw (`crate::active_ragdoll::unwrite`): the game's own locals put back and the
/// trees updated with them.
pub fn unwrite(e: &mut Engine) {
    let written = std::mem::take(&mut *WRITTEN.lock().unwrap_or_else(|p| p.into_inner()));
    for (node, (r, t)) in written.into_iter().rev() {
        write9(e, node + LOCAL_ROTATION, &r);
        write3(e, node + LOCAL_TRANSLATION, t);
    }
    let roots = std::mem::take(&mut *ROOTS.lock().unwrap_or_else(|p| p.into_inner()));
    for root in roots {
        crate::active_ragdoll::update_tree(e, root);
    }
}

/// Each body's node's world (rotation, game units).
pub type Pose = Vec<(M3, V3)>;

/// The last frame's drawn bodies (each body's node's world) and read-back (degrees, units) of
/// `actor`.
pub fn drawn(actor: u32) -> Option<(Pose, (f32, f32))> {
    ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|a| a.actor == actor)
        .map(|a| (a.drawn.clone(), a.readback))
}

/// The balance's phase and catching steps of `actor`.
pub fn state(actor: u32) -> Option<(Phase, u32, V3)> {
    ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|a| a.actor == actor)
        .and_then(|a| a.np.as_ref().map(|np| (np.phase(), a.steps, a.moved)))
}

/// Everything dropped (a load, the tests).
pub fn clear() {
    ACTORS.lock().unwrap_or_else(|p| p.into_inner()).clear();
    WRITTEN.lock().unwrap_or_else(|p| p.into_inner()).clear();
    ROOTS.lock().unwrap_or_else(|p| p.into_inner()).clear();
    *LAST.lock().unwrap_or_else(|p| p.into_inner()) = None;
    FALLS.lock().unwrap_or_else(|p| p.into_inner()).clear();
}

#[cfg(test)]
#[path = "new_physics_fixture_tests.rs"]
mod fixture_tests;
