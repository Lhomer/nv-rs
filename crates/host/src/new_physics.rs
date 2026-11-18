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
use ragdoll_core::flinch::{add, apply_m, length, scale, sub, M3, V3};
use ragdoll_core::np::Np;
use ragdoll_core::np_balance::Phase;
use ragdoll_core::np_body::Model;
use ragdoll_core::pose::Xf;
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
/// `ActorMover::Update` (`009dc120`, thiscall: the mover, the frame's time): the game's path
/// movement, held for a downed actor (`crate::knockdown` wraps it).
pub const ACTOR_MOVER_UPDATE: u32 = 0x009d_c120;

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
    np: Option<Np>,
    /// Hits waiting for its first frame (body, point in game units, impulse N·s).
    pending: Vec<(usize, V3, V3)>,
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
    /// The last frame's figures (the clips' `"np"` sample).
    now: Now,
    /// The most the capture point stood outside the feet (m) and the most a joint was softened.
    outside_most: f32,
    soft_most: f32,
    /// The steps already logged (`[np step]`).
    steps_logged: usize,
    /// Downed (M6): the label last logged, the AI's choice now and how long it has held, the
    /// rises that gave up, seconds firing from the ground.
    label: Option<&'static str>,
    choice: &'static str,
    choice_for: f32,
    fails: u32,
    fired: f32,
    /// Waiting for the main thread to hand it to the game's ragdoll (the reason: "steps",
    /// "unreachable", "path", "give up", "death"); simulated and drawn meanwhile.
    handing: Option<&'static str>,
    /// Since when (frames).
    handing_frames: u32,
    /// The last frame's animated pose (the bodies' nodes' worlds as the game had them).
    anim_last: Pose,
    /// Frames since its last hit (the per-hit logs' first frames), and that hit's outcome while
    /// it is open (`[np outcome]`).
    hit_frames: u32,
    outcome: Option<Outcome>,
    /// The bodies at the top of the body tree (no body above them): their worlds and their
    /// subtrees' are set from their locals after the update (`refresh_bodies`).
    tops: Vec<u32>,
    /// The bodies parents first (each after the body above it): the order their worlds are set
    /// in at the world pass (`at_world_pass`).
    order: Vec<usize>,
    /// Written this frame (cleared at the put-back): its worlds are set at the world pass.
    written_now: bool,
    /// The floor against the reference's height, taken at the take (game units): the animated
    /// feet's lowest point when within [`FLOOR_NEAR`] of it, else 0. Never the animated feet's
    /// each frame (Patrick's 803260d clip).
    floor_offset: Option<f32>,
    /// The world pass's lines said since the last hit.
    pass_logs: u32,
    /// Downed: the combat target's direction (game units, none without one) and the aim's angle
    /// off it (degrees) the last frame.
    aim: Option<(Option<V3>, f32)>,
    /// The ground cast under each body (where it was cast from, flat, and the height found;
    /// none: not cast, or nothing found), cast again past [`GROUND_RECAST`].
    grounds: Vec<Option<(V3, Option<f32>)>>,
    /// The actor's reference after New Physics last moved it (the game's own move since is the
    /// ground's, `Np::ground_v`).
    ref_after: Option<V3>,
    /// The floor-load lines (`[np floor load]`) said since the last hit, and the last frame's
    /// floor (game units).
    floor_logs: u32,
    floor_was: Option<f32>,
    /// What the floor taken at the take stood over the collision floor under the reference
    /// (game units; none: no collision floor found): standing, each body's ground raised by it.
    floor_lift: Option<f32>,
    /// The nodes from the 3D's root to the bodies' (each with its parent, parents first), and
    /// the snapshot of their locals right after the game's last animation update for the actor
    /// (`animation_updated`): its animation, as New Physics reads it.
    tree: Vec<(u32, u32)>,
    snap: Option<Snapshot>,
    /// The body's last frames since its last hit (the frame's time, each body's velocity and
    /// spin at its end): the hand-over's velocities, their mean.
    history: std::collections::VecDeque<(f32, Vec<(V3, V3)>)>,
    /// The animation's jumps said.
    jumps_said: u32,
    /// Each body's walls found beside it (world, metres; where the cast met it, game units):
    /// kept while the body stays within [`WALL_KEEP`] of it.
    walls: Vec<Vec<(ragdoll_core::np_body::Plane, V3)>>,
    /// The wall lines said since the last hit.
    wall_logs: u32,
}

/// The actor's animation as the game's update left it: its 3D root's world and each node's local
/// of `NpActor::tree` (in its order).
#[derive(Clone, Debug)]
struct Snapshot {
    root: Xf,
    locals: Vec<Xf>,
}

/// The hand-over's velocities come from the body's last this many frames.
const HISTORY_FRAMES: usize = 3;
/// The hand-over waits for the body inside the skeleton's limits (radians out), at most this many
/// frames.
const HAND_LIMITS: f32 = 0.09;
const HAND_WAIT_FRAMES: u32 = 45;

/// The snapshot of `root`'s animation: its world and the locals of `tree`.
fn snapshot(e: &Engine, root: u32, tree: &[(u32, u32)]) -> Snapshot {
    let parent = e.mem.u32(root + PARENT);
    let root_world = if parent == 0 {
        read_local(e, root)
    } else {
        read_world(e, parent).then(&read_local(e, root))
    };
    Snapshot {
        root: root_world,
        locals: tree.iter().map(|&(n, _)| read_local(e, n)).collect(),
    }
}

/// The bodies' nodes' worlds (rotation, game units) the snapshot makes.
fn snapshot_worlds(s: &Snapshot, tree: &[(u32, u32)], bodies: &[u32]) -> Vec<(M3, V3)> {
    let mut worlds: Vec<(u32, Xf)> = Vec::with_capacity(tree.len() + 1);
    let root = tree.first().map(|t| t.1);
    if let Some(r) = root {
        worlds.push((r, s.root));
    }
    for (&(n, p), l) in tree.iter().zip(&s.locals) {
        let up = worlds
            .iter()
            .rev()
            .find(|w| w.0 == p)
            .map_or(s.root, |w| w.1);
        worlds.push((n, up.then(l)));
    }
    bodies
        .iter()
        .map(|b| {
            worlds.iter().rev().find(|w| w.0 == *b).map_or(
                ([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], [0.0; 3]),
                |w| (w.1.r, w.1.t),
            )
        })
        .collect()
}

/// The nodes from `root` down to each of `bodies` (each with its parent), parents first.
fn tree_of(e: &Engine, root: u32, bodies: &[u32]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32, u32)> = Vec::new();
    for &b in bodies {
        let mut path = Vec::new();
        let mut at = b;
        let mut depth = 0;
        while at != 0 && at != root && depth < 32 {
            let parent = e.mem.u32(at + PARENT);
            path.push((at, parent));
            at = parent;
            depth += 1;
        }
        let total = path.len() as u32;
        for (i, (n, p)) in path.into_iter().enumerate() {
            if !out.iter().any(|o| o.0 == n) {
                out.push((n, p, total - i as u32));
            }
        }
    }
    out.sort_by_key(|o| o.2);
    out.into_iter().map(|o| (o.0, o.1)).collect()
}

/// Right after the game's animation update for `actor` (`Animation::UpdateMovement`,
/// `Animation::UpdateQueuedScenegraph`, on whichever thread runs it): the snapshot of its
/// animation New Physics reads, for an actor it has.
pub fn animation_updated(e: &mut Engine, actor: u32) {
    let mut actors = ACTORS.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(a) = actors.iter_mut().find(|a| a.actor == actor) {
        a.snap = Some(snapshot(e, a.root, &a.tree));
    }
}

/// `Animation::UpdateMovement` (`00493900`) and `Animation::UpdateQueuedScenegraph`
/// (`00493930`): the animation's actor (`pActorRef`, `+0x04`).
pub const ANIMATION_UPDATE_MOVEMENT: u32 = 0x0049_3900;
pub const ANIMATION_UPDATE_QUEUED: u32 = 0x0049_3930;
pub const ANIMATION_ACTOR: u32 = 0x04;

/// One hit's outcome as it goes (`[np outcome]`): the hit, then the most the spine's top went off
/// its animation, the most drawn lay off the body, the most body worlds set directly in a frame,
/// the catching steps before it.
#[derive(Clone, Debug)]
struct Outcome {
    impulse: f32,
    speed: f32,
    body: usize,
    steps_before: u32,
    spine_peak: f32,
    drawn_most: (f32, f32),
    set_most: u32,
    /// How it came out (a fall, downed, died) once it did; measured on until [`OUTCOME_SECONDS`]
    /// after the hit all the same (and, standing, until its steps settle).
    result: Option<String>,
    /// The frames drawn nearer the animation than the body at the world pass (after its worlds
    /// were set), and the frames something had written over the body's nodes before it (set
    /// back from the body).
    as_animation: u32,
    overwritten: u32,
    /// The most the floor carried in a frame (body weights), and a wall (N).
    floor_most: f32,
    wall_most: f32,
    /// Seconds since the hit; the pellets of its burst (1 a single hit).
    since: f32,
    pellets: u32,
}

/// A hit's outcome is measured at least this long (s), whatever came of it, before
/// `[np outcome]`, and at most [`OUTCOME_MOST`].
pub const OUTCOME_SECONDS: f32 = 2.0;
pub const OUTCOME_MOST: f32 = 6.0;

/// One frame's figures of an actor New Physics has (the clips' `"np"` sample).
#[derive(Clone, Copy, Debug, Default)]
struct Now {
    phase: &'static str,
    /// The spine top off its animation (degrees).
    spine: f32,
    /// The capture point outside the feet (m; 0 inside).
    outside: f32,
    /// The most any joint is softened (0 to `np_drive::SOFT_MOST`).
    soft: f32,
    /// The body's centre's speed against the animation's (game units a second).
    centre_speed: f32,
    /// The hips lowered for a step's reach (m).
    lower: f32,
    /// A step swinging: the foot's side (0 left, 1 right).
    swinging: Option<usize>,
    dt: f32,
}

static ACTORS: Mutex<Vec<NpActor>> = Mutex::new(Vec::new());
/// The locals written this frame: the node, the game's own (put back), and what was written (the
/// game's own is put back only over it: a local the game wrote since is its own and stays).
#[allow(clippy::type_complexity)]
static WRITTEN: Mutex<Vec<(u32, (M3, V3), (M3, V3))>> = Mutex::new(Vec::new());
static ROOTS: Mutex<Vec<(u32, Vec<u32>)>> = Mutex::new(Vec::new());
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
    /// Each body's node handed: its world rotation and position (game units).
    pub pose: Vec<(u32, M3, V3)>,
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
    let chains: Vec<(Option<usize>, Vec<u32>)> = nodes
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
    let tops: Vec<u32> = nodes
        .iter()
        .zip(&chains)
        .filter(|(_, c): &(&u32, &(Option<usize>, Vec<u32>))| c.0.is_none())
        .map(|(&n, _)| n)
        .collect();
    // Parents first: each body after the bodies above it.
    let depth = |mut b: usize| {
        let mut d = 0;
        while let Some(up) = chains[b].0 {
            b = up;
            d += 1;
            if d > 32 {
                break;
            }
        }
        d
    };
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&b| depth(b));
    let tree = tree_of(e, root, &nodes);
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
            np: None,
            pending: Vec::new(),
            since_hit: 0.0,
            drawn: Vec::new(),
            readback: (0.0, 0.0),
            readback_most: (0.0, 0.0),
            spine_peak: 0.0,
            steps: 0,
            moved: [0.0; 3],
            frames: 0,
            now: Now::default(),
            outside_most: 0.0,
            soft_most: 0.0,
            steps_logged: 0,
            label: None,
            choice: "",
            choice_for: 0.0,
            fails: 0,
            fired: 0.0,
            handing: None,
            handing_frames: 0,
            anim_last: Vec::new(),
            hit_frames: 0,
            outcome: None,
            tops,
            order,
            written_now: false,
            floor_offset: None,
            pass_logs: 0,
            aim: None,
            tree: tree.clone(),
            snap: Some(snapshot(e, root, &tree)),
            history: std::collections::VecDeque::new(),
            jumps_said: 0,
            walls: Vec::new(),
            wall_logs: 0,
            grounds: Vec::new(),
            ref_after: None,
            floor_logs: 0,
            floor_was: None,
            floor_lift: None,
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
    // Pellets of one burst (hits before a frame has run since the last): one outcome, their
    // momenta summed (each pellet closed the last as "stood, 0 frames": Patrick's 10:08 clip).
    if a.hit_frames == 0 {
        if let Some(o) = a.outcome.as_mut() {
            o.impulse += p;
            o.speed = o.impulse / m.mass;
            o.pellets += 1;
            note(format!(
                "{:08X}: [np hit] {p:.1} N s on {} (pellet {} of a burst, {:.2} m/s in all)",
                a.id, m.bodies[body].node, o.pellets, o.speed
            ));
            match a.np.as_mut() {
                Some(np) => np.hit(body, point, impulse),
                None => a.pending.push((body, point, impulse)),
            }
            return;
        }
    }
    // The last hit's outcome said as it stands now; this one's begins.
    let now = outcome_now(a);
    close_outcome(a.id, &mut a.outcome, &now);
    a.hit_frames = 0;
    a.outcome = Some(Outcome {
        pellets: 1,
        impulse: p,
        speed: p / m.mass,
        body,
        steps_before: a.steps,
        spine_peak: 0.0,
        drawn_most: (0.0, 0.0),
        set_most: 0,
        result: None,
        as_animation: 0,
        overwritten: 0,
        floor_most: 0.0,
        wall_most: 0.0,
        since: 0.0,
    });
    a.pass_logs = 0;
    a.floor_logs = 0;
    a.wall_logs = 0;
    a.history.clear();
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
    let list: Vec<NpActor> = std::mem::take(&mut *ACTORS.lock().unwrap_or_else(|p| p.into_inner()));
    if list.is_empty() {
        return;
    }
    // The game's own frame time (what its AI threads step the world by: 0 in a menu, slowed by
    // the time multiplier), never the wall clock between two draws.
    let dt = {
        let s = e.mem.f32(FRAME_SECONDS);
        if s.is_finite() {
            s.clamp(0.0, MAX_DT)
        } else {
            0.0
        }
    };
    let mut keep = Vec::new();
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
        if matches!(life, 1 | 2) && !a.drawn.is_empty() && a.handing != Some("death") {
            // Dead (downed or standing): the game's ragdoll takes the body from the pose drawn,
            // moving as New Physics' bodies were, on the main thread (`main_thread`); simulated
            // and drawn until then.
            note(format!(
                "{:08X}: [np death] {}: its ragdoll's bodies to be put on the pose drawn at the \
                 main thread's next update",
                a.id,
                a.label.map_or("standing", |l| l)
            ));
            a.handing = Some("death");
            came_out(&mut a.outcome, "died".to_string());
            a.handing_frames = 0;
        }
        if a.handing.is_none()
            && (process == 0 || e.mem.u8(process + KNOCK_STATE) != 0 || matches!(life, 1 | 2))
        {
            LET_GOS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let now = outcome_now(&a);
            close_outcome(a.id, &mut a.outcome, &format!("{now}, let go to the game"));
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
        // The animation: only the snapshot of its locals taken right after the game's animation
        // update for it (`animation_updated`), never the nodes as they are now (the game's
        // threads write them at any time, New Physics wrote them: the tester's 08:13 clip read
        // its animation flip 67, 0, 158 degrees between 7 ms frames).
        if a.snap.is_none() {
            a.snap = Some(snapshot(e, a.root, &a.tree));
        }
        let Some(anim) = a
            .snap
            .as_ref()
            .map(|s| snapshot_worlds(s, &a.tree, &a.nodes))
        else {
            continue;
        };
        // The floor under the animated feet (their capsules' lowest point) when it lies within
        // [`FLOOR_NEAR`] of the reference's height, else the reference's: a pose whose feet stand
        // a few units off the reference (Patrick's d02bbd1 clip) was pushed out of the floor
        // on its first frame, or dropped to it.
        // The floor: the reference's height and the offset taken at the take (the animated
        // feet's lowest point then, within [`FLOOR_NEAR`] of it: no kick at the take, Patrick's
        // d02bbd1 clip), never the animated feet's each frame (his 803260d clip: a walk's feet
        // took the floor off the body's, and up into them).
        let reference = read3(e, a.actor + REFR_POSITION);
        let reference_z = reference[2];
        let offset =
            match a.floor_offset {
                Some(o) => o,
                None => {
                    // The collision floor under the reference (the game's ray cast), else the
                    // reference's height; the animated feet's lowest point when within
                    // [`FLOOR_NEAR`] of it.
                    let cast = crate::active_ragdoll::floor_under(e, reference);
                    let base = cast.unwrap_or(reference_z);
                    let feet = feet_floor(&anim);
                    let (z, from) = match feet {
                        Some(z) if (z - base).abs() <= FLOOR_NEAR => (z, "the animated feet"),
                        _ if cast.is_some() => (base, "the collision floor"),
                        _ => (base, "the reference"),
                    };
                    note(format!(
                    "{:08X}: [np floor] {from} at the take: the reference's height {:+.1} units \
                     (the collision floor {}, the animated feet {}), kept",
                    a.id,
                    z - reference_z,
                    cast.map_or("not found".to_string(), |c| format!("{:+.1}", c - reference_z)),
                    feet.map_or("none".to_string(), |f| format!("{:+.1}", f - reference_z)),
                ));
                    a.floor_offset = Some(z - reference_z);
                    // Each foot's own ground lifted by the least the animated feet stand over it
                    // (one height for both, from the ground under the reference, put the uphill
                    // foot of a walk 3 cm into a slope at the take: 4.6 body weights).
                    a.floor_lift = cast.map(|c| z - c);
                    z - reference_z
                }
            };
        let floor = reference_z + offset;
        if a.np.is_none() {
            // Made on the first frame's animated pose, still: its animation's velocity is
            // measured from the frames that follow (`Np`), never from the pose at the hit.
            let mut np = Np::new(model().clone(), &anim, 1.0);
            np.keep_down = KEEP_DOWN.load(std::sync::atomic::Ordering::Relaxed);
            for (b, p, j) in a.pending.drain(..) {
                np.hit(b, p, j);
            }
            a.np = Some(np);
        }
        let Some(np) = a.np.as_mut() else { continue };
        // The ground the game itself moved the actor along since New Physics last placed it.
        np.ground_v = match a.ref_after {
            Some(was) if dt > 0.0 => {
                let d = sub(reference, was);
                scale([d[0], d[1], 0.0], 1.0 / (dt * ragdoll_core::np_body::UNITS))
            }
            _ => [0.0; 3],
        };
        // Each body's own floor, the ground cast under it (downed: never the reference's height
        // and an offset, a flat plane that follows the reference, Patrick's 23:41 clip).
        let pose = if a.drawn.len() == a.nodes.len() {
            a.drawn.clone()
        } else {
            anim.clone()
        };
        let (planes, under_pelvis, under_feet) = ground_planes(e, &mut a.grounds, &pose, floor);
        // Each body's ground raised by what the feet stood over it at the take (the floor taken
        // then, kept): its shape from the casts, never a flat plane that follows the reference
        // (downed, it pushed shins and knees into a slope; standing, it rose under a planted foot
        // as the reference walked up the slope and pushed the body up at 3 to 4 body weights).
        let lift = a.floor_lift.unwrap_or(0.0);
        let lifted = |planes: Vec<ragdoll_core::np_body::Plane>| {
            planes
                .into_iter()
                .map(|p| ragdoll_core::np_body::Plane {
                    p: add(p.p, [0.0, 0.0, lift / ragdoll_core::np_body::UNITS]),
                    n: p.n,
                })
                .collect::<Vec<_>>()
        };
        let floor = if np.down.is_some() {
            // Its movement held while it is down (the mover's at its update too).
            crate::knockdown::hold_controller(e, a.actor);
            if planes.is_empty() {
                np.floors = Vec::new();
                floor
            } else {
                np.floors = lifted(planes);
                under_pelvis + lift
            }
        } else if let (Some(_), Some(feet)) = (a.floor_lift, under_feet) {
            np.floors = lifted(planes);
            feet + lift
        } else {
            np.floors = Vec::new();
            floor
        };
        // The walls beside each body: cast along its motion from where it was (a fast body
        // through a thin wall too), kept while it stays near (Patrick's 10:08 clip: no wall was
        // ever met, "a wall 0 N" on every pose line).
        // The weapon arm (the right, holding its weapon drawn): it keeps it; the free arm pushes.
        np.weapon_side = crate::active_ragdoll::weapon_drawn(e, a.actor).then_some(1);
        let velocities: Vec<V3> = np.states.iter().map(|s| s.v).collect();
        find_walls(e, &mut a.walls, &pose, &velocities, dt);
        np.body_walls = a
            .walls
            .iter()
            .map(|ws| ws.iter().map(|w| w.0).collect())
            .collect();
        let drawn = np.frame(&anim, floor, dt);
        floor_load_said(&mut a, floor, dt);
        wall_load_said(&mut a);
        let Some(np) = a.np.as_mut() else { continue };
        a.anim_last = anim.clone();
        a.hit_frames += 1;
        if a.hit_frames <= BALANCE_LOG_FRAMES {
            let k = ragdoll_core::np_body::UNITS;
            let av = np.anim_velocity();
            let rv = np.balance.rel_v;
            note(format!(
                "{:08X}: [np balance] frame {} after the hit: dt {:.4} s, the animation {:.2} m/s, \
                 the body against it {:.2} m/s ({:.0} u/s), the capture point {:.3} m outside the \
                 feet, {}",
                a.id,
                a.hit_frames,
                dt,
                length([av[0], av[1], 0.0]),
                length([rv[0], rv[1], 0.0]),
                length([rv[0], rv[1], 0.0]) * k,
                np.balance.outside,
                phase_name(np.phase())
            ));
        }
        a.since_hit += dt;
        a.frames += 1;
        // Nothing written to the nodes here: the bodies' worlds are set at the world pass
        // (`read_drawn`), right before it skins them.
        if !drawn
            .iter()
            .all(|(r, t)| r.iter().all(|x| x.is_finite()) && t.iter().all(|x| x.is_finite()))
        {
            note(format!("{:08X}: [np let go] its pose went bad", a.id));
            continue;
        }
        roots.push((a.root, a.tops.clone()));
        a.written_now = true;
        let set_directly = a.nodes.len() as u32;
        // The body's last frames (each body's centre and turn, and the frame's time): the
        // hand-over's velocities.
        a.history
            .push_back((dt, np.states.iter().map(|s| (s.v, s.w)).collect::<Vec<_>>()));
        while a.history.len() > HISTORY_FRAMES {
            a.history.pop_front();
        }
        // Every jump of the animation said.
        if np.jumps > a.jumps_said {
            a.jumps_said = np.jumps;
            note(format!(
                "{:08X}: [np jump] the animation jumped {:.0} units in a frame{}",
                a.id,
                np.jump_last,
                if np.down.is_some() {
                    ": downed, the body kept (never put back on it)"
                } else {
                    ": the body put back on it"
                }
            ));
        }
        if let Some(o) = a.outcome.as_mut() {
            o.set_most = o.set_most.max(set_directly);
            o.since += dt;
            let weight = model().mass * ragdoll_core::np_body::GRAVITY;
            o.floor_most = o
                .floor_most
                .max(np.report.floor_load.iter().sum::<f32>() / weight);
            let spine = model().body("Bip01 Spine2").unwrap_or(0);
            o.spine_peak = o.spine_peak.max(
                ragdoll_core::np_skeleton::angle_between(&drawn[spine].0, &anim[spine].0)
                    .to_degrees(),
            );
        }
        let spine = model().body("Bip01 Spine2").unwrap_or(0);
        a.spine_peak = a.spine_peak.max(
            ragdoll_core::np_skeleton::angle_between(&drawn[spine].0, &anim[spine].0).to_degrees(),
        );
        a.drawn = drawn;
        a.steps = np.balance.taken.iter().filter(|s| !s.2).count() as u32;
        let k = ragdoll_core::np_body::UNITS;
        let soft = np
            .muscles
            .softening()
            .iter()
            .cloned()
            .fold(0.0f32, f32::max);
        let (_, com_v) = np.model.centre_of_mass(&np.states);
        a.now = Now {
            phase: if np.down.is_some() {
                "downed"
            } else {
                phase_name(np.phase())
            },
            spine: ragdoll_core::np_skeleton::angle_between(&a.drawn[spine].0, &anim[spine].0)
                .to_degrees(),
            outside: np.balance.outside.max(0.0),
            soft,
            centre_speed: length(sub(scale(com_v, k), scale(np.anim_velocity(), k))),
            lower: np.balance.lower,
            swinging: np.balance.step.as_ref().map(|s| s.side),
            dt,
        };
        a.outside_most = a.outside_most.max(a.now.outside);
        a.soft_most = a.soft_most.max(soft);
        // Each step as it lands: which foot, where, and how far the capture point still is.
        for &(side, to, stance) in np.balance.taken.iter().skip(a.steps_logged) {
            note(format!(
                "{:08X}: [np step] {} step with the {} foot to ({:.0}, {:.0}); the capture point \
                 {:.2} m outside the feet, the hips {:.2} m lowered",
                a.id,
                if stance { "a stance" } else { "a catching" },
                if side == 0 { "left" } else { "right" },
                to[0] * k,
                to[1] * k,
                np.balance.outside.max(0.0),
                np.balance.lower
            ));
        }
        a.steps_logged = np.balance.taken.len();
        if np.down.is_some() {
            let give_up = downed(e, &mut a, dt);
            if give_up {
                a.choice = "give up";
            }
        } else if a.label.is_some() {
            note(format!(
                "{:08X}: [np pose] standing again (its balance has it)",
                a.id
            ));
            a.label = None;
            a.choice = "";
            a.fails = 0;
        }
        let Some(np) = a.np.as_mut() else { continue };
        // The steps' move is the actor's, only while the pose is drawn (an actor moved under a
        // pose not drawn slid along upright: Patrick's d02bbd1 clip).
        let held = a.readback.0 <= MOVE_DRAWN.0 && a.readback.1 <= MOVE_DRAWN.1;
        let mv = if held { np.take_move() } else { [0.0; 3] };
        if length(mv) > 1e-3 {
            // New Physics owns the reference while it has the actor: moved only by it (a step's
            // end, a rise's end), its height the ground's under it (kept as far over the ground as
            // it stood at the take): released 22 units over the ground, it dropped (the tester's
            // 08:13 clip).
            let at = read3(e, a.actor + REFR_POSITION);
            // Swept: never through a wall or a crate on the way, and its capsule clear of them
            // where it ends (Patrick's 10:08 and 10:10 clips: moved 193 and 213 units in one
            // frame, through what lay between).
            let (to_flat, said) = swept_move(e, a.actor, at, mv);
            let moved = sub(to_flat, at);
            let left = sub(mv, [moved[0], moved[1], 0.0]);
            if length(left) > 0.5 {
                np.give_back(scale(left, 1.0 / ragdoll_core::np_body::UNITS));
            }
            if let Some(why) = said {
                note(format!(
                    "{:08X}: [np move] the actor moved {:.0} of {:.0} units to the body: {why}",
                    a.id,
                    length([moved[0], moved[1], 0.0]),
                    length(mv)
                ));
            }
            let mv = [moved[0], moved[1], 0.0];
            let mut to = add(at, mv);
            let over = a.floor_lift.unwrap_or(0.0) - a.floor_offset.unwrap_or(0.0);
            if let Some(g) = crate::active_ragdoll::floor_under(e, [to[0], to[1], at[2] + 30.0]) {
                to[2] = g + over;
            }
            crate::knockdown::place(e, a.actor, to);
            // The height it was given too: the animation comes moved by it (never a jump).
            let dz = read3(e, a.actor + REFR_POSITION)[2] - at[2];
            np.anim_moved([0.0, 0.0, dz / ragdoll_core::np_body::UNITS]);
            a.moved = add(a.moved, mv);
        }
        // Its movement held while New Physics has it (the mover's at its update too).
        crate::knockdown::hold_controller(e, a.actor);
        a.ref_after = Some(read3(e, a.actor + REFR_POSITION));
        if a.handing.is_none()
            && ((np.phase() == Phase::Falling && np.down.is_none()) || a.choice == "give up")
        {
            // Out of steps, past them, or the downed AI's rises given up: the game's ragdoll
            // takes it, on the main thread (`main_thread`: never at the draw's set-up, where its
            // AI threads use the task queue); simulated and drawn until then, never let go.
            let reason = if a.choice == "give up" {
                "give up"
            } else {
                match np.balance.fall_reason {
                    "" => "steps",
                    r => r,
                }
            };
            note(format!(
                "{:08X}: [np fall] {reason}: {} catching steps, the capture point {:.2} m outside \
                 the feet; to the game's ragdoll at the main thread's next update",
                a.id, a.steps, np.balance.outside
            ));
            a.handing = Some(reason);
            came_out(&mut a.outcome, format!("fell ({reason})"));
            a.handing_frames = 0;
        }
        // Waiting for the game's ragdoll: the body brought inside the skeleton's own limits (the
        // ragdoll's constraints), so the game takes the pose it is drawn in.
        np.to_limits = a.handing.is_some();
        if a.handing.is_some() {
            a.handing_frames += 1;
            close_if_due(&mut a);
            keep.push(a);
            continue;
        }
        let calm = np.down.is_none()
            && a.label.is_none()
            && np.phase() == Phase::Standing
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
                summary(&mut a);
                continue;
            }
        }
        close_if_due(&mut a);
        keep.push(a);
    }
    ROOTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .extend(roots);
    let mut actors = ACTORS.lock().unwrap_or_else(|p| p.into_inner());
    // Claimed during the frame (a hit while this ran): kept too.
    keep.append(&mut actors);
    // Only actors still downed have their shots withheld.
    WITHHELD
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .retain(|w| keep.iter().any(|a| a.actor == *w && a.label.is_some()));
    *actors = keep;
}

/// The game's frame seconds (`011dea30`: what `Main::OnIdle_UpdateTimer` keeps for its AI
/// threads' world step, the time multiplier applied, 0 in a menu).
const FRAME_SECONDS: u32 = 0x011d_ea30;
/// The task queue's test (true while the game's AI threads use it: `KnockExplosion` and the
/// bodies' setters wait for the main thread).
const AI_THREAD: u32 = 0x008c_7aa0;
/// At the take the floor is taken from the animated feet when they stand within this of the
/// collision floor (game units).
const FLOOR_NEAR: f32 = 12.0;
/// The actor is moved by the body only while drawn within this of it (degrees, units).
const MOVE_DRAWN: (f32, f32) = (1.0, 1.0);

/// The lowest point of the animated feet's capsules (game units), from each body's node's world.
fn feet_floor(anim: &[(M3, V3)]) -> Option<f32> {
    let m = model();
    let k = ragdoll_core::np_body::UNITS;
    ["Bip01 L Foot", "Bip01 R Foot"]
        .iter()
        .filter_map(|n| m.body(n))
        .filter_map(|b| {
            let (r, t) = anim.get(b)?;
            let def = &m.bodies[b];
            def.ends
                .iter()
                .map(|e| add(*t, apply_m(r, scale(*e, k)))[2] - def.radius * k)
                .reduce(f32::min)
        })
        .reduce(f32::min)
}

/// The bodies the ground is cast under (each foot, each calf: the knees, the pelvis, each hand).
const GROUND_SAMPLES: [&str; 7] = [
    "Bip01 L Foot",
    "Bip01 R Foot",
    "Bip01 L Calf",
    "Bip01 R Calf",
    "Bip01 NonAccum",
    "Bip01 L Hand",
    "Bip01 R Hand",
];
/// A sample moved this far (flat, game units) since its cast is cast again.
const GROUND_RECAST: f32 = 6.0;
/// The steepest ground taken (its slope's tangent), and how much a few samples in a line lean
/// to flat (game units squared, each).
const GROUND_STEEPEST: f32 = 1.2;
const GROUND_FLAT_BIAS: f32 = 4.0;
/// A cast more than this off the fitted ground (game units) lies on another level.
const GROUND_STEP: f32 = 4.0;

/// Each body's own floor (world, metres) from the ground cast under [`GROUND_SAMPLES`] in `pose`
/// (each body's node's world, game units), cached and cast again past [`GROUND_RECAST`]: the
/// plane fitted through them (its slope, leaning to flat for samples in a line), through the
/// cast nearest each body; `flat` (game units) where nothing was found. Also the ground under
/// the pelvis and under the feet (their mean; game units).
fn ground_planes(
    e: &mut Engine,
    cache: &mut Vec<Option<(V3, Option<f32>)>>,
    pose: &[(M3, V3)],
    flat: f32,
) -> (Vec<ragdoll_core::np_body::Plane>, f32, Option<f32>) {
    let m = model();
    let k = ragdoll_core::np_body::UNITS;
    cache.resize(GROUND_SAMPLES.len(), None);
    let mut found: Vec<V3> = Vec::new();
    let mut pelvis = flat;
    let mut feet = Vec::new();
    for (i, name) in GROUND_SAMPLES.iter().enumerate() {
        let Some(at) = m.body(name).and_then(|b| pose.get(b)).map(|p| p.1) else {
            continue;
        };
        let stale = cache[i].map_or(true, |(xy, _)| {
            length([at[0] - xy[0], at[1] - xy[1], 0.0]) > GROUND_RECAST
        });
        if stale {
            cache[i] = Some((
                [at[0], at[1], 0.0],
                crate::active_ragdoll::floor_under(e, at),
            ));
        }
        if let Some((xy, Some(z))) = cache[i] {
            found.push([xy[0], xy[1], z]);
            if *name == "Bip01 NonAccum" {
                pelvis = z;
            }
            if name.ends_with("Foot") {
                feet.push(z);
            }
        }
    }
    let feet = (!feet.is_empty()).then(|| feet.iter().sum::<f32>() / feet.len() as f32);
    if found.is_empty() {
        return (Vec::new(), flat, None);
    }
    // z = a (x - x0) + b (y - y0) + c, least squares, leaning to flat.
    let fit = |pts: &[V3]| -> (V3, f32, f32) {
        let n = pts.len() as f32;
        let mean = scale(pts.iter().fold([0.0; 3], |s, p| add(s, *p)), 1.0 / n);
        let (mut sxx, mut syy, mut sxy, mut sxz, mut syz) = (0.0f32, 0.0, 0.0, 0.0, 0.0);
        for p in pts {
            let (x, y, z) = (p[0] - mean[0], p[1] - mean[1], p[2] - mean[2]);
            sxx += x * x;
            syy += y * y;
            sxy += x * y;
            sxz += x * z;
            syz += y * z;
        }
        sxx += GROUND_FLAT_BIAS * n;
        syy += GROUND_FLAT_BIAS * n;
        let det = sxx * syy - sxy * sxy;
        let (mut a, mut b) = if det.abs() > 1e-6 {
            ((sxz * syy - syz * sxy) / det, (syz * sxx - sxz * sxy) / det)
        } else {
            (0.0, 0.0)
        };
        let steep = (a * a + b * b).sqrt();
        if steep > GROUND_STEEPEST {
            a *= GROUND_STEEPEST / steep;
            b *= GROUND_STEEPEST / steep;
        }
        (mean, a, b)
    };
    let off = |p: &V3, (mean, a, b): (V3, f32, f32)| {
        p[2] - (mean[2] + a * (p[0] - mean[0]) + b * (p[1] - mean[1]))
    };
    // The level most casts lie on: a cast more than GROUND_STEP off the fit lies on another (a
    // slab's top, a crate's, a pit): fitted without those, it never tilts or lifts the rest, and
    // a body over one stands on it flat (one plane through all tilted at a slab's edge and pushed
    // a body into it: Patrick's 10:08 clip). On one even ground nothing changes.
    let mut plane = fit(&found);
    let mut other = vec![false; found.len()];
    for _ in 0..2 {
        let flags: Vec<bool> = found
            .iter()
            .map(|p| off(p, plane).abs() > GROUND_STEP)
            .collect();
        let level: Vec<V3> = found
            .iter()
            .zip(&flags)
            .filter(|(_, o)| !**o)
            .map(|(p, _)| *p)
            .collect();
        if !flags.iter().any(|o| *o) || level.len() < 3 {
            break;
        }
        other = flags;
        plane = fit(&level);
    }
    let (_, a, b) = plane;
    let normal = ragdoll_core::flinch::unit([-a, -b, 1.0]).unwrap_or([0.0, 0.0, 1.0]);
    let planes = pose
        .iter()
        .map(|(_, t)| {
            let near = found
                .iter()
                .enumerate()
                .min_by(|(_, p), (_, q)| {
                    let d = |c: &V3| length([c[0] - t[0], c[1] - t[1], 0.0]);
                    d(p).total_cmp(&d(q))
                })
                .map(|(i, p)| (*p, other[i]));
            let (at, flat_here) = near.unwrap_or(([t[0], t[1], flat], false));
            ragdoll_core::np_body::Plane {
                p: scale(at, 1.0 / k),
                n: if flat_here { [0.0, 0.0, 1.0] } else { normal },
            }
        })
        .collect();
    (planes, pelvis, feet)
}

/// The heights over the reference a move is swept at (game units: the knees', the waist's).
const SWEEP_HEIGHTS: [f32; 2] = [12.0, 30.0];

/// The actor's move from `at` by `mv` (flat, game units), swept at [`SWEEP_HEIGHTS`]: stopped its
/// capsule's radius short of what it meets, then nudged clear of anything within its radius
/// around (eight casts, twice). Answers where it may stand (flat; `at`'s height) and what it
/// met, if anything.
fn swept_move(e: &mut Engine, actor: u32, at: V3, mv: V3) -> (V3, Option<String>) {
    let radius = crate::knockdown::capsule(e, actor);
    let dist = length([mv[0], mv[1], 0.0]);
    if dist < 1e-3 {
        return (at, None);
    }
    let dir = [mv[0] / dist, mv[1] / dist, 0.0];
    let mut allowed = dist;
    let mut said = None;
    for h in SWEEP_HEIGHTS {
        let from = add(at, [0.0, 0.0, h]);
        if let Some(hit) = crate::active_ragdoll::cast_world(e, from, dir, dist + radius) {
            let d = length(sub(hit, from));
            if d - radius < allowed {
                allowed = (d - radius).max(0.0);
                said = Some(format!(
                    "a wall {:.0} units along the way at {:.0} units up",
                    d, h
                ));
            }
        }
    }
    let mut to = add(at, scale(dir, allowed));
    let mut nudged = 0.0f32;
    for _ in 0..2 {
        let mut push = [0.0f32; 3];
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::TAU / 8.0;
            let d8 = [a.cos(), a.sin(), 0.0];
            for h in SWEEP_HEIGHTS {
                let from = add(to, [0.0, 0.0, h]);
                if let Some(hit) = crate::active_ragdoll::cast_world(e, from, d8, radius) {
                    let d = length(sub(hit, from));
                    if d < radius {
                        push = sub(push, scale(d8, (radius - d) / 2.0));
                    }
                }
            }
        }
        if length(push) < 0.1 {
            break;
        }
        nudged += length(push);
        to = add(to, push);
    }
    if nudged > 0.1 {
        let was = said.take().map_or(String::new(), |s| format!("{s}; "));
        said = Some(format!(
            "{was}nudged {nudged:.0} units clear of what is beside it"
        ));
    }
    (to, said)
}

/// A wall found is kept while its body stays within this of where the cast met it (game units),
/// at most [`WALLS_MOST`] a body; a body casts along its motion past [`WALL_CAST_SPEED`] (m/s),
/// as far as it goes in [`WALL_LOOK`] seconds and its radius more.
const WALL_KEEP: f32 = 40.0;
const WALLS_MOST: usize = 3;
const WALL_CAST_SPEED: f32 = 0.2;
const WALL_LOOK: f32 = 0.1;
/// The side rays that find a wall's face (game units across the cast).
const WALL_SIDE: f32 = 5.0;
/// A surface whose normal rises more than this (under 50 degrees) is ground: the floors have it.
const WALL_WALKABLE: f32 = 0.64;

/// Each body's walls: cast from its capsule's centre in `pose` (game units) along its flat
/// velocity (`velocities`, m/s), the face's normal from two side rays (else against the cast);
/// those it moved away from dropped.
fn find_walls(
    e: &mut Engine,
    walls: &mut Vec<Vec<(ragdoll_core::np_body::Plane, V3)>>,
    pose: &[(M3, V3)],
    velocities: &[V3],
    dt: f32,
) {
    let m = model();
    let k = ragdoll_core::np_body::UNITS;
    walls.resize(pose.len(), Vec::new());
    for (b, (r, t)) in pose.iter().enumerate() {
        let def = &m.bodies[b];
        let centre = add(*t, apply_m(r, scale(def.com, k)));
        walls[b].retain(|w| length([w.1[0] - centre[0], w.1[1] - centre[1], 0.0]) <= WALL_KEEP);
        let v = velocities.get(b).copied().unwrap_or([0.0; 3]);
        let flat = [v[0], v[1], 0.0];
        let speed = length(flat);
        if speed < WALL_CAST_SPEED {
            continue;
        }
        let dir = scale(flat, 1.0 / speed);
        let range = (speed * (dt + WALL_LOOK) + def.radius) * k + 4.0;
        let Some(hit) = crate::active_ragdoll::cast_world(e, centre, dir, range) else {
            continue;
        };
        // The surface's normal from three points: the hit, one to each side, and one raised
        // (a ray each the same way); one a body could stand on is the floor's, not a wall.
        let across = [-dir[1], dir[0], 0.0];
        let ray = |e: &mut Engine, off: V3| {
            crate::active_ragdoll::cast_world(e, add(centre, off), dir, range + 20.0)
        };
        let sides = (
            ray(e, scale(across, WALL_SIDE)),
            ray(e, scale(across, -WALL_SIDE)),
        );
        let up = ray(e, [0.0, 0.0, WALL_SIDE]);
        let along = match sides {
            (Some(p1), Some(p2)) => sub(p1, p2),
            _ => across,
        };
        let rise = match up {
            Some(pu) => sub(pu, hit),
            None => [0.0, 0.0, 1.0],
        };
        let n = ragdoll_core::flinch::unit(ragdoll_core::flinch::cross(along, rise))
            .unwrap_or(scale(dir, -1.0));
        let n = if ragdoll_core::flinch::dot(n, dir) > 0.0 {
            scale(n, -1.0)
        } else {
            n
        };
        if n[2] > WALL_WALKABLE {
            continue;
        }
        let plane = ragdoll_core::np_body::Plane {
            p: scale(hit, 1.0 / k),
            n,
        };
        let same = walls[b].iter().any(|w| {
            ragdoll_core::flinch::dot(w.0.n, n) > 0.95
                && ragdoll_core::flinch::dot(sub(scale(hit, 1.0 / k), w.0.p), n).abs() < 0.04
        });
        if !same {
            if walls[b].len() >= WALLS_MOST {
                walls[b].remove(0);
            }
            walls[b].push((plane, hit));
        }
    }
}

/// A wall's load on the body said (`[np wall]`, a few a hit): the bodies against it and how
/// hard.
fn wall_load_said(a: &mut NpActor) {
    let Some(np) = a.np.as_ref() else {
        return;
    };
    let load = &np.report.wall_load;
    let total: f32 = load.iter().sum();
    if total <= 1.0 {
        return;
    }
    if let Some(o) = a.outcome.as_mut() {
        o.wall_most = o.wall_most.max(total);
    }
    if a.wall_logs >= FLOOR_LOGS {
        return;
    }
    a.wall_logs += 1;
    let weight = model().mass * ragdoll_core::np_body::GRAVITY;
    let bodies = load
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > 1.0)
        .map(|(b, n)| {
            format!(
                "{} {:.0} N",
                model().bodies[b].node.trim_start_matches("Bip01 "),
                n
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    note(format!(
        "{:08X}: [np wall] {:.0} N ({:.2} body weights) against a wall: {bodies}",
        a.id,
        total,
        total / weight
    ));
}

/// The floor's load past [`FLOOR_LOAD_SAID`] body weights in a frame said with its source
/// (`[np floor load]`, a few a hit): the bodies that carried it, what the balance was doing
/// (a step landing, planted feet, downed), and how far the floor moved since the last frame.
const FLOOR_LOAD_SAID: f32 = 3.0;
const FLOOR_LOGS: u32 = 6;

fn floor_load_said(a: &mut NpActor, floor: f32, dt: f32) {
    let was = a.floor_was.replace(floor);
    let Some(np) = a.np.as_ref() else {
        return;
    };
    let weight = model().mass * ragdoll_core::np_body::GRAVITY;
    let load = &np.report.floor_load;
    let total: f32 = load.iter().sum();
    if total / weight <= FLOOR_LOAD_SAID || a.floor_logs >= FLOOR_LOGS {
        return;
    }
    a.floor_logs += 1;
    let mut top: Vec<(usize, f32)> = load.iter().copied().enumerate().collect();
    top.sort_by(|x, y| y.1.total_cmp(&x.1));
    let bodies = top
        .iter()
        .take(3)
        .map(|(b, n)| {
            format!(
                "{} {:.1}",
                model().bodies[*b].node.trim_start_matches("Bip01 "),
                n / weight
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let bal = &np.balance;
    let doing = if np.down.is_some() {
        "downed".to_string()
    } else if let Some(st) = bal.step {
        if st.t >= st.time {
            format!(
                "a step landing ({} foot pressed down)",
                if st.side == 0 { "left" } else { "right" }
            )
        } else {
            "a step swinging".to_string()
        }
    } else {
        format!(
            "{}, {} feet planted",
            phase_name(np.phase()),
            bal.plants.iter().filter(|p| p.is_some()).count()
        )
    };
    let (_, com_v) = np.model.centre_of_mass(&np.states);
    note(format!(
        "{:08X}: [np floor load] {:.1} body weights (dt {dt:.3}): {bodies}; {doing}; the floor \
         moved {:+.1} units since the last frame; the body's centre at {:+.2} m/s up and down, \
         {:.2} m/s across; the fastest body {:.2} m/s",
        a.id,
        total / weight,
        was.map_or(0.0, |w| floor - w),
        com_v[2],
        length([com_v[0], com_v[1], 0.0]),
        np.states.iter().map(|s| length(s.v)).fold(0.0f32, f32::max)
    ));
}

/// `[np balance]` for a take's first frames, `[np readback]` and `[np drawn]` for its first.
const BALANCE_LOG_FRAMES: u32 = 6;
const READBACK_LOG_FRAMES: u32 = 3;
/// The times New Physics let an actor go (the tests': a fall is never let go).
static LET_GOS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub fn let_gos() -> u32 {
    LET_GOS.load(std::sync::atomic::Ordering::Relaxed)
}

/// On the main thread's update (`crate::knockdown::frame`, while the task queue is free): each
/// actor waiting for the game's ragdoll is handed to it. A fall or a give-up: `KnockExplosion` at
/// strength 0 and its bodies on the pose drawn, moving as New Physics' bodies were
/// (`knockdown::hand_over`); if the game refuses it, New Physics keeps it downed. A death: its
/// ragdoll's bodies put on the pose drawn with those velocities (`knockdown::place_bodies`).
pub fn main_thread(e: &mut Engine) {
    let waiting: Vec<NpActor> = {
        let mut actors = ACTORS.lock().unwrap_or_else(|p| p.into_inner());
        if !actors.iter().any(|a| a.handing.is_some()) {
            return;
        }
        let (go, stay): (Vec<NpActor>, Vec<NpActor>) = std::mem::take(&mut *actors)
            .into_iter()
            .partition(|a| a.handing.is_some() && a.np.is_some() && !a.drawn.is_empty());
        *actors = stay;
        go
    };
    if e.call(AI_THREAD, &[]).bool() {
        ACTORS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .extend(waiting);
        return;
    }
    let mut back = Vec::new();
    for mut a in waiting {
        let reason = a.handing.unwrap_or("steps");
        let Some(np) = a.np.as_mut() else { continue };
        // Not yet inside the skeleton's limits (brought in a few degrees a frame): the game's
        // ragdoll would snap it there; wait, a little while at most.
        // A give-up or a death waits for it (no hurry); a fall from standing goes at once.
        let out = np.report.limit_most;
        let wait = if matches!(reason, "give up" | "death") {
            HAND_WAIT_FRAMES
        } else {
            0
        };
        if (!np.on_data_limits || out > HAND_LIMITS) && a.handing_frames < wait {
            back.push(a);
            continue;
        }
        // What is left out projected in at once: the pose handed lies inside them.
        let (inside, out) = np.into_limits();
        let left = model()
            .bodies
            .iter()
            .enumerate()
            .map(|(b, _)| {
                (
                    ragdoll_core::np_skeleton::angle_between(&inside[b].0, &a.drawn[b].0)
                        .to_degrees(),
                    length(sub(inside[b].1, a.drawn[b].1)),
                )
            })
            .fold((0.0f32, 0.0f32), |m, x| (m.0.max(x.0), m.1.max(x.1)));
        a.drawn = inside;
        let anim = if a.anim_last.is_empty() {
            a.drawn.clone()
        } else {
            a.anim_last.clone()
        };
        let (shown, animated, velocities) = handing(&a.nodes, &a.drawn, &anim, &a.history);
        // The hand-over's source said: the pose drawn against the animation the game had its
        // bodies at (a downed body handed over lies far off a standing animation: the 23:41
        // clip's give-up was drawn 180 degrees, 122 units off).
        let apart = shown
            .iter()
            .zip(&animated)
            .map(|(d, an)| {
                (
                    ragdoll_core::np_skeleton::angle_between(&d.1, &an.1).to_degrees(),
                    length(sub(d.2, an.2)),
                )
            })
            .fold((0.0f32, 0.0f32), |m, x| (m.0.max(x.0), m.1.max(x.1)));
        note(format!(
            "{:08X}: [np hand-over] {reason}: the pose drawn at most {:.0} degrees, {:.0} units off \
             the animation the game had its ragdoll's bodies at; brought inside the skeleton's \
             limits ({:.1} degrees out after, the last {:.1} degrees, {:.1} units at the hand-over); \
             its bodies moving as over New Physics' last {HISTORY_FRAMES} frames ({} frames after \
             it was asked)",
            a.id,
            apart.0,
            apart.1,
            out.to_degrees(),
            left.0,
            left.1,
            a.handing_frames
        ));
        let k = ragdoll_core::np_body::UNITS;
        // The root body's and the centre's speed as handed (the frames' mean).
        let pelvis = velocities.get(model().root).map_or(0.0, |v| length(v.1));
        let com_v = {
            let m = model();
            let mut p = [0.0f32; 3];
            for (b, v) in velocities.iter().enumerate() {
                p = add(p, scale(v.1, m.bodies[b].mass / k));
            }
            scale(p, 1.0 / m.mass)
        };
        if reason == "death" {
            let (moved, given) =
                crate::knockdown::place_bodies(e, a.actor, &shown, &animated, &velocities);
            note(format!(
                "{:08X}: [np death] its ragdoll's {moved} bodies on the pose drawn, {given} moving \
                 as New Physics' were ({} frames after it died)",
                a.id, a.handing_frames
            ));
            DEATHS
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push((a.actor, moved, given));
            summary(&mut a);
            continue;
        }
        match crate::knockdown::hand_over(e, a.actor, &shown, &animated, &velocities) {
            Ok(n) => {
                note(format!(
                    "{:08X}: [np fall] {reason}: handed to its ragdoll {} frames after the fall: \
                     {n} bodies moving on; the root body at {pelvis:.0} units/s, the body's \
                     centre {:.0} units/s",
                    a.id,
                    a.handing_frames,
                    length(com_v) * k
                ));
                FALLS.lock().unwrap_or_else(|p| p.into_inner()).push(Fall {
                    actor: a.actor,
                    root: pelvis,
                    centre: length(com_v) * k,
                    pose: shown.iter().map(|x| (x.0, x.1, x.2)).collect(),
                });
                summary(&mut a);
            }
            Err(why) => {
                // Never let go on a fall: kept, downed (M6).
                note(format!(
                    "{:08X}: [np fall] {reason}: the game's ragdoll refuses it ({why}): New \
                     Physics keeps it, downed",
                    a.id
                ));
                np.keep_down = true;
                a.handing = None;
                if a.choice == "give up" {
                    a.choice = "";
                    a.fails = 0;
                }
                back.push(a);
            }
        }
    }
    ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .extend(back);
}

/// The world pass's lines (`[np drawn]`, `[np locals]`) said for each hit's first frames, and for
/// at most this many frames after them in which something wrote over the body.
const PASS_LOG_FRAMES: u32 = 8;

/// Each body's node's world against `drawn` (the most degrees and units off), and whether a body
/// off its animation by over 2 degrees or 1 unit is drawn nearer its animation (`anim`) than the
/// body.
fn drawn_against(
    e: &Engine,
    nodes: &[u32],
    drawn: &[(M3, V3)],
    anim: &[(M3, V3)],
) -> ((f32, f32), bool) {
    let mut most = (0.0f32, 0.0f32);
    let mut as_anim = false;
    for (b, &node) in nodes.iter().enumerate() {
        let w = read_world(e, node);
        let deg = ragdoll_core::np_skeleton::angle_between(&w.r, &drawn[b].0).to_degrees();
        let u = length(sub(w.t, drawn[b].1));
        most = (most.0.max(deg), most.1.max(u));
        if let Some(an) = anim.get(b) {
            let off = ragdoll_core::np_skeleton::angle_between(&drawn[b].0, &an.0).to_degrees();
            let off_u = length(sub(drawn[b].1, an.1));
            let to_anim = ragdoll_core::np_skeleton::angle_between(&w.r, &an.0).to_degrees()
                + length(sub(w.t, an.1));
            if (off > 2.0 || off_u > 1.0) && to_anim < deg + u {
                as_anim = true;
            }
        }
    }
    (most, as_anim)
}

/// At the world pass's first call (`crate::active_ragdoll::world_pass`, right before it culls and
/// skins the actors): each body's node's world set from New Physics' body itself, parents first,
/// and every node under it from it with its own local (`crate::active_ragdoll::set_subtree`),
/// never from the bodies' locals (Patrick's 803260d clip: the locals written over between the
/// write and the draw, 446 of 1170 downed frames were drawn as the animation). Before, what wrote
/// over the write is measured and said (`[np locals]`: the body locals changed since the write
/// and how many are the animation's again; the worlds off the body); after, the worlds are
/// checked against the body (`[np drawn]`).
pub fn read_drawn(e: &mut Engine) {
    let mut actors = ACTORS.lock().unwrap_or_else(|p| p.into_inner());
    for a in actors.iter_mut() {
        if !a.written_now || a.drawn.len() != a.nodes.len() {
            continue;
        }
        // What the nodes hold as the pass begins.
        let (before, _) = drawn_against(e, &a.nodes, &a.drawn, &a.anim_last);
        // The body locals the game wrote since the snapshot New Physics read its animation from.
        let mut changed = 0u32;
        if let Some(snap) = a.snap.as_ref() {
            for &node in &a.nodes {
                let Some(i) = a.tree.iter().position(|t| t.0 == node) else {
                    continue;
                };
                let now = read_local(e, node);
                let was = snap.locals[i];
                if now.r != was.r || now.t != was.t {
                    changed += 1;
                }
            }
        }
        let over = changed > 0;
        // Set from the body.
        for &b in &a.order {
            let node = a.nodes[b];
            let world = Xf {
                r: a.drawn[b].0,
                t: a.drawn[b].1,
                s: e.mem.f32(node + WORLD_SCALE),
            };
            crate::active_ragdoll::set_subtree(e, node, world, 0);
        }
        let (after, as_anim) = drawn_against(e, &a.nodes, &a.drawn, &a.anim_last);
        a.readback = after;
        a.readback_most = (
            a.readback_most.0.max(after.0),
            a.readback_most.1.max(after.1),
        );
        if let Some(o) = a.outcome.as_mut() {
            o.drawn_most = (o.drawn_most.0.max(after.0), o.drawn_most.1.max(after.1));
            o.as_animation += u32::from(as_anim);
            o.overwritten += u32::from(over);
        }
        let first = a.hit_frames <= READBACK_LOG_FRAMES;
        if a.outcome.is_some() && (first || (over && a.pass_logs < PASS_LOG_FRAMES)) {
            if !first {
                a.pass_logs += 1;
            }
            note(format!(
                "{:08X}: [np locals] frame {} after the hit: {changed} of {} body locals written \
                 by the game since the animation's snapshot (never read), the game's worlds {:.2} \
                 degrees, {:.2} units off the body before they were set",
                a.id,
                a.hit_frames,
                a.nodes.len(),
                before.0,
                before.1
            ));
            note(format!(
                "{:08X}: [np drawn] frame {} after the hit: the {} bodies' worlds set from the \
                 body at the world pass: drawn against simulated at most {:.2} degrees, {:.2} \
                 units{}",
                a.id,
                a.hit_frames,
                a.nodes.len(),
                after.0,
                after.1,
                if as_anim {
                    ", drawn as the animation"
                } else {
                    ""
                }
            ));
        }
    }
}

/// After the world pass (`crate::active_ragdoll::world_pass`): the bodies' nodes checked against
/// New Physics' body again: anything that wrote over them while the pass drew (the AI threads)
/// is said and counted.
pub fn after_world_pass(e: &mut Engine) {
    let mut actors = ACTORS.lock().unwrap_or_else(|p| p.into_inner());
    for a in actors.iter_mut() {
        if !a.written_now || a.drawn.len() != a.nodes.len() {
            continue;
        }
        let (off, as_anim) = drawn_against(e, &a.nodes, &a.drawn, &a.anim_last);
        a.readback = (a.readback.0.max(off.0), a.readback.1.max(off.1));
        a.readback_most = (a.readback_most.0.max(off.0), a.readback_most.1.max(off.1));
        if off.0 <= 1.0 && off.1 <= 1.0 {
            continue;
        }
        if let Some(o) = a.outcome.as_mut() {
            o.drawn_most = (o.drawn_most.0.max(off.0), o.drawn_most.1.max(off.1));
            o.as_animation += u32::from(as_anim);
        }
        if a.outcome.is_some() && a.pass_logs < PASS_LOG_FRAMES {
            a.pass_logs += 1;
            note(format!(
                "{:08X}: [np drawn] frame {} after the hit: written over during the world pass: \
                 {:.2} degrees, {:.2} units off the body after it{}",
                a.id,
                a.hit_frames,
                off.0,
                off.1,
                if as_anim { " (as the animation)" } else { "" }
            ));
        }
    }
}

/// A downed body (M6) is kept rather than handed to the game's ragdoll (tests of the M4
/// hand-over turn it off).
static KEEP_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
#[cfg(test)]
pub(crate) fn set_keep_down(on: bool) {
    KEEP_DOWN.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The deaths New Physics handed to the game's ragdoll (actor, bodies placed, given velocities).
static DEATHS: Mutex<Vec<(u32, usize, usize)>> = Mutex::new(Vec::new());
pub fn deaths() -> Vec<(u32, usize, usize)> {
    DEATHS.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// The downed actors whose shots are withheld now (they cannot aim from the pose they have).
static WITHHELD: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Downed, how long the body settles before the AI chooses (s), how long it fires from the
/// ground before it tries to stand (s), its shots' extra spread (as a support hand this far off a
/// two-handed grip, `crate::one_hand`), the rises that may give up before it collapses into the
/// game's ragdoll (the knockdowns' get-up).
pub const DOWN_SETTLE: f32 = 0.7;
pub const FIRE_DOWN: f32 = 4.0;
pub const DOWN_SPREAD: f32 = 0.6;
pub const DOWN_TRIES: u32 = 3;
/// After a rise gives up, the AI waits this long before it tries again (s).
pub const RISE_AGAIN: f32 = 1.0;

/// The actor's combat target: its vtable's `GetCombatTarget` (slot `0x42c`, as the game's own
/// `GetCombatTarget` script function calls it). Not a field: `actor + 0x42c` held 480.0
/// (`0x43f00000`) in Patrick's tester logs, and its position read past the end of memory (the
/// 20:58 and 23:01 crashes in the draw on b507e997).
const COMBAT_TARGET_SLOT: u32 = 0x42c;

/// `actor`'s combat target ([`COMBAT_TARGET_SLOT`]), only a mapped reference the game's form
/// table knows by its own form id, never `actor` itself.
fn combat_target(e: &mut Engine, actor: u32) -> Option<u32> {
    let vtable = e.mem.u32(actor);
    if vtable == 0 || !e.mem.is_mapped(vtable + COMBAT_TARGET_SLOT) {
        return None;
    }
    if e.mem.u32(vtable + COMBAT_TARGET_SLOT) == 0 {
        return None;
    }
    let target = e.vcall(actor, COMBAT_TARGET_SLOT, &args![]).u32();
    if target == 0 || target == actor || target % 4 != 0 {
        return None;
    }
    let end = target.checked_add(REFR_POSITION + 12)?;
    if !(e.mem.is_mapped(target) && e.mem.is_mapped(end)) {
        return None;
    }
    let id = e.mem.u32(target + FORM_ID);
    (id != 0 && e.call(LOOKUP_FORM, &args![id]).u32() == target).then_some(target)
}

/// Whether `actor` is downed and its shots are withheld (it cannot aim from the pose it has):
/// `crate::gunplay`'s fire hook skips the shot.
pub fn withholds_fire(actor: u32) -> bool {
    WITHHELD
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(&actor)
}

/// The label of `actor`'s downed pose (none while it stands).
pub fn label_of(actor: u32) -> Option<&'static str> {
    ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|a| a.actor == actor)
        .and_then(|a| a.label)
}

/// Whether New Physics has `actor` downed (the game's path movement is held for it).
pub fn is_down(actor: u32) -> bool {
    ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|a| a.actor == actor && a.label.is_some())
}

/// The pose to hand to the game's ragdoll: each body's node with its drawn world (`drawn`), its
/// animated world (`animated`: where the game kept the bodies), and its body's velocities (game
/// units a second, rad/s).
#[allow(clippy::type_complexity)]
fn handing(
    nodes: &[u32],
    drawn: &[(M3, V3)],
    animated: &[(M3, V3)],
    history: &std::collections::VecDeque<(f32, Vec<(V3, V3)>)>,
) -> (Vec<(u32, M3, V3)>, Vec<(u32, M3, V3)>, Vec<(u32, V3, V3)>) {
    let k = ragdoll_core::np_body::UNITS;
    // Each body's velocity and spin, the mean over its last frames since the hit (one frame's
    // was its last substep's, a constraint's kick in it: the knockdown's clamp fired at 2813
    // units a second, the tester's 08:13 clip).
    let n = history.len().max(1) as f32;
    let velocities = nodes
        .iter()
        .enumerate()
        .map(|(b, &node)| {
            let (v, w) = history.iter().fold(([0.0; 3], [0.0; 3]), |m, h| {
                h.1.get(b).map_or(m, |x| (add(m.0, x.0), add(m.1, x.1)))
            });
            (node, scale(v, k / n), scale(w, 1.0 / n))
        })
        .collect();
    let shown = nodes
        .iter()
        .zip(drawn)
        .map(|(&n, d)| (n, d.0, d.1))
        .collect();
    let animated = nodes
        .iter()
        .zip(animated)
        .map(|(&n, d)| (n, d.0, d.1))
        .collect();
    (shown, animated, velocities)
}

/// The downed AI's frame (M6): the pose's label logged as it changes (`[np pose]`), the reads,
/// and a choice from them (`[np choice]`): settle; fire from the ground (the weapon arm free and
/// the target within the shoulder's reach; its shots wider); stand (the controller pursues
/// balance); wait. Answers true when the rises have given up [`DOWN_TRIES`] times: the collapse
/// goes to the game's ragdoll and the knockdowns' get-up.
fn downed(e: &mut Engine, a: &mut NpActor, dt: f32) -> bool {
    let Some(np) = a.np.as_mut() else {
        return false;
    };
    let Some(d) = np.down.as_ref() else {
        return false;
    };
    let label = d.labeler.label.name();
    let reading = d.reading;
    let since = d.since;
    let failed = d.rise_failed;
    let push = np.push();
    let to_target = combat_target(e, a.actor).map(|target| {
        let at = read3(e, target + REFR_POSITION);
        let me = read3(e, a.actor + REFR_POSITION);
        sub(at, me)
    });
    let rd = ragdoll_core::np_down::reads(&reading, to_target, push);
    a.aim = Some((to_target, rd.aim_off));
    if a.label.is_none() {
        // Kept downed (M6): the fall said as the game's ragdoll's would be.
        let reason = match np.balance.fall_reason {
            "" => "steps",
            r => r,
        };
        note(format!(
            "{:08X}: [np fall] {reason}: kept downed by New Physics (the capture point {:.2} m \
             outside the feet)",
            a.id, np.balance.outside
        ));
        came_out(&mut a.outcome, format!("downed ({reason})"));
    }
    if a.label != Some(label) {
        a.label = Some(label);
        note(format!(
            "{:08X}: [np pose] {label}: pelvis {:.0} u up, torso tilted {:.0} degrees, feet {:.0} N, \
             seat {:.0} N, hands {:.0} N, a wall {:.0} N; weapon arm {}, target {:.0} degrees off, \
             push {:.0} N for {:.0} N",
            a.id,
            reading.pelvis_height * ragdoll_core::np_body::UNITS,
            reading.tilt.to_degrees(),
            reading.feet,
            reading.seat,
            reading.hands,
            reading.back_wall,
            if rd.arm_free { "free" } else { "carrying" },
            rd.aim_off,
            push,
            reading.weight
        ));
    }
    if failed && a.choice == "stand" {
        a.fails += 1;
        if let Some(d) = np.down.as_mut() {
            d.rise_failed = false;
        }
    }
    let choice = if since < DOWN_SETTLE {
        "settle"
    } else if a.fails >= DOWN_TRIES {
        "give up"
    } else if rd.can_aim && to_target.is_some() && a.fired < FIRE_DOWN {
        "fire"
    } else if a.choice == "wait" && a.fails > 0 && a.choice_for < RISE_AGAIN {
        "wait"
    } else if failed {
        // A rise gave up this frame: a moment's rest before the next.
        "wait"
    } else if np.rise || rd.can_push_up {
        "stand"
    } else {
        "wait"
    };
    if choice == "fire" {
        a.fired += dt;
        crate::one_hand::set(a.actor, DOWN_SPREAD);
    }
    np.rise = choice == "stand";
    np.firing = choice == "fire";
    {
        let mut w = WITHHELD.lock().unwrap_or_else(|p| p.into_inner());
        w.retain(|&x| x != a.actor);
        if choice != "fire" {
            w.push(a.actor);
        }
    }
    if choice != a.choice {
        note(format!(
            "{:08X}: [np choice] {choice} (from {label}; aim {}, push up {}, crawl {}, a target {}, \
             rises given up {}{})",
            a.id,
            rd.can_aim,
            rd.can_push_up,
            rd.can_crawl,
            to_target.is_some(),
            a.fails,
            if choice == "fire" {
                format!(", shots wider as a hand {DOWN_SPREAD} off the grip")
            } else {
                String::new()
            }
        ));
        a.choice = choice;
        a.choice_for = 0.0;
    }
    a.choice_for += dt;
    choice == "give up"
}

/// A reaction's summary when New Physics lets go of an actor.
/// Stood, or stepped N (the catching steps since the open hit).
fn stood_or_stepped(a: &NpActor) -> String {
    let before = a.outcome.as_ref().map_or(0, |o| o.steps_before);
    match a.steps.saturating_sub(before) {
        0 => "stood".to_string(),
        n => format!("stepped {n}"),
    }
}

/// How the open hit came out now: what it came to (a fall, downed, died) once it did, else downed,
/// stood or stepped N.
fn outcome_now(a: &NpActor) -> String {
    match a.outcome.as_ref().and_then(|o| o.result.clone()) {
        Some(r) => r,
        None if a.label.is_some() => "downed".to_string(),
        None => stood_or_stepped(a),
    }
}

/// The open hit's outcome said once it has been measured [`OUTCOME_SECONDS`] and its steps have
/// settled (a fall, downed or dead, or standing again with no step under way), or at
/// [`OUTCOME_MOST`] whatever (the testers' b507e997 logs: "stood" was said at 2 s, then three
/// catching steps walked it into a fall unsaid).
fn close_if_due(a: &mut NpActor) {
    let settled = a.np.as_ref().map_or(true, |np| {
        np.down.is_some()
            || (np.phase() == Phase::Standing
                && np.balance.step.is_none()
                && np.balance.since >= ragdoll_core::np_balance::SETTLE)
    });
    if a.outcome.as_ref().is_some_and(|o| {
        o.since >= OUTCOME_MOST || (o.since >= OUTCOME_SECONDS && (settled || o.result.is_some()))
    }) {
        let now = outcome_now(a);
        close_outcome(a.id, &mut a.outcome, &now);
    }
}

/// The open hit came to `result` (the first thing it came to is kept); it is measured on.
fn came_out(outcome: &mut Option<Outcome>, result: String) {
    if let Some(o) = outcome.as_mut() {
        o.result.get_or_insert(result);
    }
}

/// The open hit's outcome said (`[np outcome]`) with `result`, and closed.
fn close_outcome(id: u32, outcome: &mut Option<Outcome>, result: &str) {
    let Some(o) = outcome.take() else {
        return;
    };
    note(format!(
        "{:08X}: [np outcome] hit {:.1} N s / {:.2} m/s ({} pellets) on {} -> {result}, a wall \
         at most {:.0} N, peak spine {:.1} \
         deg, drawn-vs-sim max {:.2} deg / {:.2} u at the world pass, {} frames drawn as the \
         animation, {} frames written over before the world pass (set back), world set \
         directly {} bones, floor at most {:.1} body weights, over {:.2} s",
        id,
        o.impulse,
        o.speed,
        o.pellets,
        model().bodies[o.body].node.trim_start_matches("Bip01 "),
        o.wall_most,
        o.spine_peak,
        o.drawn_most.0,
        o.drawn_most.1,
        o.as_animation,
        o.overwritten,
        o.set_most,
        o.floor_most,
        o.since
    ));
}

/// Each body tree's top worked out from its parent's world and its own local, and every node
/// under it from it (`crate::active_ragdoll::set_subtree`, the old system's in-game fallback):
/// `NiAVObject::Update` hands a body node's world to its `bhkBlendCollisionObject`, which in the
/// no-controller update's mode (the thread block's `+0x2c0` at 3) does nothing, so the update
/// left FNV's 18 body nodes as the animation had them (Patrick's d02bbd1 clip).
fn refresh_bodies(e: &mut Engine, tops: &[u32]) {
    for &top in tops {
        let parent = e.mem.u32(top + PARENT);
        if parent == 0 {
            continue;
        }
        let world = read_world(e, parent).then(&read_local(e, top));
        crate::active_ragdoll::set_subtree(e, top, world, 0);
    }
}

fn summary(a: &mut NpActor) {
    let result = outcome_now(a);
    close_outcome(a.id, &mut a.outcome, &result);
    note(format!(
        "{:08X}: [np reaction] {} frames ({:.2} s since its last hit): spine top at most {:.1} \
         degrees off its animation, joints softened at most {:.2}, the capture point at most {:.2} \
         m outside the feet, {} catching steps, the actor moved {:.0} units; drawn against \
         simulated at most {:.2} degrees and {:.2} units (want 0.5 and 0.5)",
        a.id,
        a.frames,
        a.since_hit,
        a.spine_peak,
        a.soft_most,
        a.outside_most,
        a.steps,
        length(a.moved),
        a.readback_most.0,
        a.readback_most.1
    ));
}

fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Standing => "standing",
        Phase::Stepping => "stepping",
        Phase::Recovering => "recovering",
        Phase::Falling => "falling",
    }
}

/// The clips' sample of an actor New Physics has (`crate::room_record`): `"np":{...}` with its
/// balance's phase, its downed pose's label and the AI's choice (null standing), the spine top off its animation (degrees), the capture point outside the feet
/// (m), the most a joint is softened, the body's centre's speed against the animation's (units a
/// second), the hips lowered (m), the foot swinging (0 left, 1 right, or null), the catching steps,
/// the actor's move (units), the read-back (degrees, units: drawn against simulated), the seconds
/// since its last hit and the frame's time (s); `"np":null` when New Physics does not have it.
pub fn sample_field(actor: u32) -> String {
    let actors = ACTORS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(a) = actors.iter().find(|a| a.actor == actor) else {
        return "\"np\":null".to_string();
    };
    let n = &a.now;
    format!(
        "\"np\":{{\"phase\":{},\"label\":{},\"choice\":{},\"spine\":{},\"outside\":{},\"soft\":{},\"centre_speed\":{},\"lower\":{},\"swinging\":{},\"steps\":{},\"moved\":{},\"readback\":[{},{}],\"since_hit\":{},\"dt\":{}}}",
        room_events::text(if n.phase.is_empty() { "taken" } else { n.phase }),
        a.label.map_or("null".to_string(), room_events::text),
        if a.choice.is_empty() {
            "null".to_string()
        } else {
            room_events::text(a.choice)
        },
        room_events::num(n.spine),
        room_events::num(n.outside),
        room_events::num(n.soft),
        room_events::num(n.centre_speed),
        room_events::num(n.lower),
        n.swinging.map_or("null".to_string(), |s| s.to_string()),
        a.steps,
        room_events::num(length(a.moved)),
        room_events::num(a.readback.0),
        room_events::num(a.readback.1),
        room_events::num(a.since_hit),
        room_events::num(n.dt)
    )
}

/// After the draw (`crate::active_ragdoll::unwrite`): the game's own locals put back and the
/// trees updated with them.
pub fn unwrite(e: &mut Engine) {
    let written = std::mem::take(&mut *WRITTEN.lock().unwrap_or_else(|p| p.into_inner()));
    for (node, (r, t), (wr, wt)) in written.into_iter().rev() {
        // Only over what was written: a local the game wrote since (its animation's pass on
        // its AI threads) is its own and newer than ours.
        if read9(e, node + LOCAL_ROTATION) == wr && read3(e, node + LOCAL_TRANSLATION) == wt {
            write9(e, node + LOCAL_ROTATION, &r);
            write3(e, node + LOCAL_TRANSLATION, t);
        }
    }
    for a in ACTORS.lock().unwrap_or_else(|p| p.into_inner()).iter_mut() {
        a.written_now = false;
    }
    let roots = std::mem::take(&mut *ROOTS.lock().unwrap_or_else(|p| p.into_inner()));
    for (root, tops) in roots {
        crate::active_ragdoll::update_tree(e, root);
        // The body nodes back on the game's locals too (the update leaves them as written).
        refresh_bodies(e, &tops);
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

/// The body's centre's speed against its animation's now (m/s, flat), for `actor`.
pub fn rel_speed(actor: u32) -> Option<f32> {
    ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|a| a.actor == actor)
        .and_then(|a| a.np.as_ref())
        .map(|np| {
            let v = np.balance.rel_v;
            length([v[0], v[1], 0.0])
        })
}

/// The floor's load on `actor`'s body over the last frame (N, the frame's mean, every body's).
pub fn floor_force(actor: u32) -> Option<f32> {
    ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|a| a.actor == actor)
        .and_then(|a| a.np.as_ref())
        .map(|np| np.report.floor_load.iter().sum())
}

/// Downed, `actor`'s combat target's direction and the aim's angle off it (the last frame).
pub fn aim_of(actor: u32) -> Option<(Option<V3>, f32)> {
    ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|a| a.actor == actor)
        .and_then(|a| a.aim)
}

/// The tests': `actor`'s strength left (`Np::effort`: 0, its legs cannot push it up).
#[cfg(test)]
pub(crate) fn set_effort(actor: u32, effort: f32) {
    if let Some(np) = ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter_mut()
        .find(|a| a.actor == actor)
        .and_then(|a| a.np.as_mut())
    {
        np.effort = effort;
    }
}

/// The tests': `actor`'s downed AI has given up its rises (as after [`DOWN_TRIES`] that failed),
/// so its next frame hands it to the game's ragdoll.
#[cfg(test)]
pub(crate) fn give_up(actor: u32) {
    if let Some(a) = ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter_mut()
        .find(|a| a.actor == actor)
    {
        a.fails = DOWN_TRIES;
    }
}

/// Each body's spin now (rad/s, world) of `actor` (the tests' measure of a turn its motion
/// explains).
pub fn spins(actor: u32) -> Option<Vec<V3>> {
    ACTORS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|a| a.actor == actor)
        .and_then(|a| a.np.as_ref())
        .map(|np| np.states.iter().map(|s| s.w).collect())
}

/// What the tests read of `actor` each frame: its catches (length, how far outside the planned
/// landing left the capture point; m), each hand's plant (game units) and whether it has
/// touched, the floor's load on each hand (N), each hand's centre (game units), the weapon arm,
/// and whether the legs push in a rise.
#[cfg(test)]
pub(crate) struct Probe {
    pub catches: Vec<(f32, f32)>,
    pub hands: [Option<V3>; 2],
    pub landed: [bool; 2],
    pub hand_load: [f32; 2],
    pub hand_at: [V3; 2],
    pub hand_low: [f32; 2],
    pub weapon_side: Option<usize>,
    pub lifting: bool,
}

#[cfg(test)]
pub(crate) fn probe(actor: u32) -> Option<Probe> {
    let actors = ACTORS.lock().unwrap_or_else(|p| p.into_inner());
    let np = actors.iter().find(|a| a.actor == actor)?.np.as_ref()?;
    let arms = np.bodies.arms;
    let load = |b: usize| np.report.floor_load.get(b).copied().unwrap_or(0.0);
    let k = ragdoll_core::np_body::UNITS;
    Some(Probe {
        catches: np.balance.catches.clone(),
        hands: np.down.as_ref().map_or([None, None], |d| d.hand_plants),
        landed: np.down.as_ref().map_or([false, false], |d| d.hand_landed),
        hand_load: [load(arms[0][2]), load(arms[1][2])],
        // Each hand's node (its plant's point) and its lowest point's height (game units).
        hand_at: [0, 1].map(|s| {
            let st = &np.states[arms[s][2]];
            let com = np.model.bodies[arms[s][2]].com;
            scale(sub(st.x, ragdoll_core::active::qrot(st.q, com)), k)
        }),
        hand_low: [0, 1].map(|s| {
            let b = arms[s][2];
            let st = &np.states[b];
            let def = &np.model.bodies[b];
            def.ends
                .iter()
                .map(|e| add(st.x, ragdoll_core::active::qrot(st.q, sub(*e, def.com)))[2])
                .fold(f32::INFINITY, f32::min)
                * k
        }),
        weapon_side: np.weapon_side,
        lifting: np.down.as_ref().is_some_and(|d| d.lifting),
    })
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
    FALLS.lock().unwrap_or_else(|p| p.into_inner()).clear();
    WITHHELD.lock().unwrap_or_else(|p| p.into_inner()).clear();
    DEATHS.lock().unwrap_or_else(|p| p.into_inner()).clear();
}

#[cfg(test)]
#[path = "new_physics_fixture_tests.rs"]
mod fixture_tests;
