//! New Physics' actors on Delirium (`delirium_bridge`), where it runs (the player's own GTA IV
//! named in `sDeliriumGame`, its start logged): a human New Physics takes on a hit is given to a
//! Delirium agent made from its pose, with its own profile, the shot given to it, and each frame
//! its parts posed onto FNV's bones (`delirium_bridge::rig`) become the bodies New Physics draws.
//! Delirium keeps the body for the whole fall (Patrick's plan; never the game's ragdoll, alive,
//! downed, rising or dead): downed, it lies as Delirium has it while New Physics' downed controller
//! reads it and times its settle; rising, Delirium's muscles follow New Physics' rise (the arms
//! pushing off through planted hands) until it is upright and still, and then the game's animation
//! has it again; dead, it is Delirium's corpse. Anything Delirium refuses lets New Physics take the
//! actor as it is, said in the log; every change of who has it is said too.
//!
//! Nothing of this runs where Delirium doesn't (CI, every PC without GTA IV): there New Physics
//! is exactly as before. The conversions between FNV's bones and Delirium's parts are tested here.

use delirium_bridge::agent::{Agent, Body, PartState, World};
use delirium_bridge::delirium::prelude::{Quat, Transform, Vec3};
use delirium_bridge::rig::Rig;
use ragdoll_core::flinch::{M3, V3};
use ragdoll_core::np_body::{Edge, Plane, UNITS};
use std::sync::OnceLock;

/// FNV's human skeleton as the rig has it: its names, parents, bind worlds (metres) and each
/// bone's place under its parent in the bind pose (metres, in the parent's frame).
pub(crate) struct Skel {
    pub names: Vec<String>,
    pub parents: Vec<Option<usize>>,
    pub rest: Vec<Transform>,
    pub local_t: Vec<Vec3>,
}

pub(crate) fn skel() -> &'static Skel {
    static S: OnceLock<Skel> = OnceLock::new();
    S.get_or_init(|| {
        let sk = ragdoll_core::np_skeleton::Skeleton::fnv();
        let rest = delirium_bridge::rig::fnv_bind(&sk);
        let parents: Vec<Option<usize>> = sk.nodes.iter().map(|n| n.parent).collect();
        let local_t = (0..rest.len())
            .map(|b| match parents[b] {
                None => rest[b].translation,
                Some(p) => rest[p].rotation.inverse() * (rest[b].translation - rest[p].translation),
            })
            .collect();
        Skel {
            names: sk.nodes.iter().map(|n| n.name.clone()).collect(),
            parents,
            rest,
            local_t,
        }
    })
}

/// A node's world (rotation, game units) as Delirium's transform: metres from `origin` (game
/// units), the rotation as a quaternion.
pub(crate) fn to_tf(r: &M3, t: V3, origin: V3) -> Transform {
    let [w, x, y, z] = ragdoll_core::active::qfrom_m(&ragdoll_core::pose::ortho(r));
    Transform::from_translation(Vec3::new(
        (t[0] - origin[0]) / UNITS,
        (t[1] - origin[1]) / UNITS,
        (t[2] - origin[2]) / UNITS,
    ))
    .with_rotation(Quat::from_xyzw(x, y, z, w).normalize())
}

/// [`to_tf`] back: a world in game units.
pub(crate) fn from_tf(tf: &Transform, origin: V3) -> (M3, V3) {
    let q = tf.rotation.normalize();
    let r = ragdoll_core::active::m_from_q([q.w, q.x, q.y, q.z]);
    let t = tf.translation * UNITS;
    (r, [t.x + origin[0], t.y + origin[1], t.z + origin[2]])
}

/// FNV's bones' worlds (metres from the origin) for Delirium's parts at `parts`: the rig's pose,
/// down the skeleton from its root, the whole skeleton moved so the pelvis is where the rig puts
/// it.
// Each bone indexes its turn, its parent's and its local place together.
#[allow(clippy::needless_range_loop)]
pub(crate) fn bones_from_parts(rig: &Rig, parts: &[Transform]) -> Vec<Transform> {
    let s = skel();
    let (local, hips) = rig.pose(parts);
    let n = s.names.len();
    // Each bone's turn in the world first.
    let mut turn = vec![Quat::IDENTITY; n];
    for b in 0..n {
        turn[b] = match s.parents[b] {
            None => local[b],
            Some(p) => (turn[p] * local[b]).normalize(),
        };
    }
    // The pelvis's parent (New Physics' root body, `Bip01 NonAccum`) is no part of Delirium's: it
    // turns with the pelvis, as it lies over it in the bind pose. Left at its bind turn, a lying
    // body's root body stood upright over its hips and the spine under it was placed as if the
    // body stood (up to 9.5 units off).
    let h = rig.hips();
    if let Some(p) = s.parents[h] {
        let under = s.rest[p].rotation.inverse() * s.rest[h].rotation;
        turn[p] = (turn[h] * under.inverse()).normalize();
    }
    // Then each bone's place under its parent's turn.
    let mut w = vec![Transform::IDENTITY; n];
    for b in 0..n {
        let at = match s.parents[b] {
            None => s.local_t[b],
            Some(p) => w[p].translation + turn[p] * s.local_t[b],
        };
        w[b] = Transform::from_translation(at).with_rotation(turn[b]);
    }
    let shift = hips - w[h].translation;
    for t in &mut w {
        t.translation += shift;
    }
    w
}

/// New Physics' bodies' node worlds (rotation, game units) for Delirium's parts at `parts`.
pub(crate) fn bodies_from_parts(rig: &Rig, parts: &[Transform], origin: V3) -> Vec<(M3, V3)> {
    let s = skel();
    let bones = bones_from_parts(rig, parts);
    crate::new_physics::model()
        .bodies
        .iter()
        .map(|b| {
            s.names.iter().position(|n| *n == b.node).map_or(
                ([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], origin),
                |i| from_tf(&bones[i], origin),
            )
        })
        .collect()
}

/// What New Physics found around the body (metres, world), as one mesh from `origin` (game
/// units): each body's floor and walls as squares where they were met, the step edges and crates.
pub(crate) fn world_mesh(
    (floors, ground): (&[Plane], Option<&delirium_bridge::world::Mesh>),
    walls: &[Vec<Plane>],
    edges: &[Edge],
    centres: &[V3],
    floor: f32,
    origin: V3,
) -> delirium_bridge::world::Mesh {
    use delirium_bridge::world::{edge, plane, Mesh};
    let o = [origin[0] / UNITS, origin[1] / UNITS, origin[2] / UNITS];
    let local = |p: V3| [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
    let mut m = Mesh::default();
    // The ground cast as one height field where it was (else each body's floor as a square).
    if let Some(g) = ground {
        m.add(g);
    }
    for (b, c) in centres.iter().enumerate() {
        let c = local([c[0] / UNITS, c[1] / UNITS, c[2] / UNITS]);
        if let (None, Some(pl)) = (ground, floors.get(b)) {
            m.add(&plane(
                &Plane {
                    p: local(pl.p),
                    n: pl.n,
                },
                c,
                1.0,
            ));
        }
        for pl in walls.get(b).into_iter().flatten() {
            m.add(&plane(
                &Plane {
                    p: local(pl.p),
                    n: pl.n,
                },
                c,
                0.75,
            ));
        }
    }
    for ed in edges {
        m.add(&edge(
            &Edge {
                p: local(ed.p),
                ..*ed
            },
            floor - o[2],
            0.3,
        ));
    }
    if floors.is_empty() && ground.is_none() {
        // No floor found per body: the floor under the reference.
        let c = [0.0, 0.0, floor - o[2]];
        m.add(&plane(
            &Plane {
                p: c,
                n: [0.0, 0.0, 1.0],
            },
            c,
            3.0,
        ));
    }
    m
}

/// Whether `actor`'s corpse is one Delirium keeps (its ragdoll held on the pose drawn: the
/// game's loot, drag and VATS find it where it lies).
pub(crate) fn corpse_held(e: &engine::prelude::Engine, actor: u32) -> bool {
    let c = e.mem.u32(actor + RAGDOLL_CONTROLLER);
    c != 0
        && CORPSES
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(&c)
}

/// `bhkRagdollController::DisableRagdollAnim` (`00c7c150`): `Actor::Kill` turns a body's ragdoll
/// loose with it (its bodies stop following the pose drawn). The actor's ragdoll controller is at
/// `actor + 0xac`.
pub(crate) const DISABLE_RAGDOLL_ANIM: u32 = 0x00c7_c150;
const RAGDOLL_CONTROLLER: u32 = 0xac;

/// The ragdoll controllers of the corpses Delirium keeps: the game's death leaves their ragdolls
/// following the pose drawn (Delirium's body with its muscles off).
static CORPSES: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// [`DISABLE_RAGDOLL_ANIM`] over the game's: nothing for a corpse Delirium keeps (but JIP's
/// OnRagdoll seam at its entry, asked as the game's is), the game's own for every other ragdoll.
fn disable_ragdoll_anim(e: &mut engine::prelude::Engine, a: &[u32]) -> engine::prelude::Ret {
    let controller = a.first().copied().unwrap_or(0);
    let kept = CORPSES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains(&controller);
    if kept {
        if e.hook_users(engine_core::extend::Hook::OnRagdoll) != 0 {
            e.site(DISABLE_RAGDOLL_ANIM + 1, &[controller]);
        }
    } else {
        engine::units::bshavok::bhkragdollcontroller::bhk_ragdoll_controller_disable_ragdoll_anim(
            e,
            engine::prelude::Ptr::new(controller),
            a.get(1).copied().unwrap_or(0) as u8,
        );
    }
    engine::prelude::Ret::default()
}

/// Where Delirium runs: the corpses' hold over the game's death ([`disable_ragdoll_anim`]).
pub(crate) fn install(e: &mut engine::prelude::Engine) {
    e.register(DISABLE_RAGDOLL_ANIM, disable_ragdoll_anim);
}

/// A corpse's hold: its controller listed while it lives, unlisted when dropped.
pub(crate) struct CorpseHold(u32);

impl CorpseHold {
    fn of(e: &engine::prelude::Engine, actor: u32) -> Option<CorpseHold> {
        let c = e.mem.u32(actor + RAGDOLL_CONTROLLER);
        if c == 0 {
            return None;
        }
        CORPSES.lock().unwrap_or_else(|p| p.into_inner()).push(c);
        Some(CorpseHold(c))
    }
}

impl Drop for CorpseHold {
    fn drop(&mut self) {
        let mut list = CORPSES.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(i) = list.iter().position(|&c| c == self.0) {
            list.remove(i);
        }
    }
}

/// What New Physics asks of a body on Delirium: the real one (Delirium's agent, its body and its
/// world: [`RealBody`]) where Delirium runs, or, in the tests, a stand-in
/// (`crate::delirium_stand_in`: the integration's frames, hand-overs and checks run in CI without
/// GTA IV). Metres from the origin, Z up; each call as the bridge's (`delirium_bridge::agent`).
pub(crate) trait DlBody: Send {
    fn set_mesh(&mut self, mesh: &delirium_bridge::world::Mesh) -> Result<(), String>;
    fn shot(
        &mut self,
        part: usize,
        at: Vec3,
        dir: Vec3,
        health: f32,
        force: f32,
    ) -> Result<(), String>;
    fn blast(&mut self, at: Vec3, radius: f32, speed: f32) -> Result<(), String>;
    fn kill(&mut self) -> Result<(), String>;
    fn step(&mut self, dt: f32) -> Result<(), String>;
    fn parts(&self) -> Result<Vec<Transform>, String>;
    fn balancing(&self) -> bool;
    fn should_get_up(&self, after: f32) -> bool;
    fn relax(&mut self) -> Result<(), String>;
    fn rise(&mut self, on: bool, help: f32) -> Result<(), String>;
    fn follow(&mut self, parts: &[Transform]) -> Result<(), String>;
    fn set_part(&mut self, part: usize, state: &PartState) -> Result<(), String>;
}

/// Delirium's own body: its agent, its build (the actor's profile's), its world.
pub(crate) struct RealBody {
    agent: Agent,
    _body: Body,
    world: World,
}

impl DlBody for RealBody {
    fn set_mesh(&mut self, mesh: &delirium_bridge::world::Mesh) -> Result<(), String> {
        self.world.set_mesh(1, mesh)
    }
    fn shot(
        &mut self,
        part: usize,
        at: Vec3,
        dir: Vec3,
        health: f32,
        force: f32,
    ) -> Result<(), String> {
        self.agent
            .shot_force(&self.world, part, at, dir, health, force)
    }
    fn blast(&mut self, at: Vec3, radius: f32, speed: f32) -> Result<(), String> {
        self.agent.explosion(&self.world, at, radius, speed)
    }
    fn kill(&mut self) -> Result<(), String> {
        self.agent.kill()
    }
    fn step(&mut self, dt: f32) -> Result<(), String> {
        self.agent.step(&self.world, dt)
    }
    fn parts(&self) -> Result<Vec<Transform>, String> {
        self.agent.parts()
    }
    fn balancing(&self) -> bool {
        self.agent.balancing()
    }
    fn should_get_up(&self, after: f32) -> bool {
        self.agent.should_get_up(after)
    }
    fn relax(&mut self) -> Result<(), String> {
        self.agent.relax()
    }
    fn rise(&mut self, on: bool, help: f32) -> Result<(), String> {
        self.agent.rise(&self.world, on, help)
    }
    fn follow(&mut self, parts: &[Transform]) -> Result<(), String> {
        self.agent.follow(parts)
    }
    fn set_part(&mut self, part: usize, state: &PartState) -> Result<(), String> {
        self.agent.set_part(part, state)
    }
}

/// Whether bodies go to Delirium: where it runs, or in a test with its stand-in on.
pub(crate) fn running() -> bool {
    #[cfg(test)]
    if crate::delirium_stand_in::on() {
        return true;
    }
    delirium_bridge::running()
}

/// The rig (FNV's bones on Delirium's parts) bodies are made with: Delirium's, or the stand-in's.
fn rig() -> Option<&'static Rig> {
    #[cfg(test)]
    if crate::delirium_stand_in::on() {
        return Some(crate::delirium_stand_in::rig());
    }
    delirium_bridge::started().map(|s| &s.rig)
}

/// A body for `states` with `profile`'s settings: Delirium's, or the test's stand-in.
fn make_body(
    states: &[PartState],
    profile: &delirium_bridge::profile::Profile,
) -> Result<Box<dyn DlBody>, String> {
    #[cfg(test)]
    if crate::delirium_stand_in::on() {
        return Ok(crate::delirium_stand_in::body(states));
    }
    let st = delirium_bridge::started().ok_or("Delirium isn't running")?;
    let body = Body::new(&st.build, &st.frames, profile.kg)?;
    let agent = Agent::new(&body, states, profile)?;
    Ok(Box::new(RealBody {
        agent,
        _body: body,
        world: World::new()?,
    }))
}

/// One actor's Delirium: its agent, its own body (its profile's) and its world, the origin its
/// places are measured from (the reference at the take), and how long it has stood balanced.
pub(crate) struct Dl {
    agent: Box<dyn DlBody>,
    rig: &'static Rig,
    pub origin: V3,
    pub frames: u32,
    balanced_for: f32,
    /// The pelvis over the floor at the take (game units): standing.
    standing: f32,
    /// The last frame's drawn bodies (for the speed and the hand-over's velocities).
    last: Vec<(M3, V3)>,
    /// Time not yet stepped (s, under [`delirium_bridge::agent::STEP`]) and the parts of the last
    /// two steps: each frame is drawn between them, so the body moves smoothly at any frame rate
    /// and is stepped in Delirium's own whole steps (Patrick's 22:38 log: at his frame rate every
    /// other frame repeated the last step's pose, its speed reading 0 and then twice its own).
    pending: f32,
    steps: (Vec<Transform>, Vec<Transform>),
    /// Dead: its muscles off for good, its ragdoll held to the pose drawn while this lives.
    corpse: Option<CorpseHold>,
    /// Its muscles off (dead, after any wait).
    killed: bool,
    /// Died mid-rise: frames of its wait so far, its muscles still on New Physics' pose while the
    /// limbs are brought inside the skeleton's limits (Patrick: a death mid-rise keeps the wait and
    /// brings the limbs in).
    pub dying: Option<u32>,
    /// What has its frame: Delirium's own reactions, or its muscles on New Physics' get-up.
    pub own: Own,
    /// The ground under and around it as the game's collision has it (a height field cast at
    /// [`GROUND_N`] by [`GROUND_N`] points, metres from the origin), and where it was cast (game
    /// units): cast again once the pelvis is [`GROUND_AGAIN`] from there.
    ground: Option<(V3, delirium_bridge::world::Mesh)>,
    /// Frames in a row its pose was not drawn (a one-frame jump held at the last right pose).
    glitch: u32,
    /// Down or dead: the most a joint of the drawn pose was out of the skeleton's limits before it
    /// was drawn inside them (radians).
    pub limited: f32,
    /// Down or dead: how far the drawn pose's limits have come in (New Physics' passes, growing
    /// [`LIMIT_RAMP`] a second from none each time it lies down), so a pose far out (a rise's squat
    /// cut back down) is brought in over a few frames, never in one.
    limit_in: f32,
    /// How long New Physics' bodies have been the body (rising or held standing: its pose drawn),
    /// s; none otherwise.
    np_for: f32,
    /// Just back on Delirium's own body from New Physics' rise pose (a rise cut back down): each
    /// body's offset from Delirium's pose to the one last drawn (a turn and a move, game units),
    /// and how much of it is left, eased out over [`EASE_BACK`].
    ease: Option<(Vec<(M3, V3)>, f32)>,
    /// Rising or held standing: New Physics' pose of the frame before (its parts' velocities).
    target: Option<Vec<(M3, V3)>>,
    /// Rising or held standing: New Physics' rise pose its parts are put on each step, and their
    /// velocities (m/s): the body Delirium's, its get-up New Physics' (its planted hands, its
    /// timings; Delirium's muscles alone never lifted it: Patrick's 23:16 clip).
    guide: Option<(Vec<Transform>, Vec<Vec3>)>,
    /// Seconds since this Delirium was made (after [`RETAKES_BACK_AFTER`] without a refusal the
    /// retakes are counted from none again).
    pub age: f32,
    /// The joint that was farthest out of the skeleton's limits (for the log).
    worst: &'static str,
}

/// The ground's height field: points a side, metres apart (3.5 m square around the pelvis).
pub(crate) const GROUND_N: usize = 11;
pub(crate) const GROUND_SPACING: f32 = 0.35;
/// The pelvis this far from where the ground was cast (game units): cast again.
pub(crate) const GROUND_AGAIN: f32 = 55.0;
/// A joint more out of the skeleton's limits than this (radians, 3 degrees): the pose brought in.
pub(crate) const LIMIT_HOLD: f32 = 0.052;
/// Frames in a row a jump is held at the last right pose before Delirium is refused.
pub(crate) const GLITCHES: u32 = 3;
/// The pelvis help of Delirium's muscle rise (of its weight): enough to lift the body with its
/// muscles on New Physics' rise (Delirium's own half a weight left it lying while New Physics'
/// rise stood: Patrick's 15:30 log, "standing" with the pelvis 21 of 66 units up).
pub(crate) const RISE_HELP: f32 = 1.2;
/// A body taken again moves at most this fast (m/s, each part): New Physics' step from a pose
/// deep in its walls had parts at many metres a second (Patrick's 17:34 clip: a light hit on a
/// prone body, refused, taken again and thrown yards).
pub(crate) const RETAKE_MOST: f32 = 2.0;
/// Delirium this long with a body (s) without a refusal: its retakes counted from none again.
pub(crate) const RETAKES_BACK_AFTER: f32 = 5.0;
/// The skeleton's limits on the pose drawn: at most this many of New Physics' passes (each turns a
/// joint at most 0.01 rad in), enough to bring in a joint 100 degrees out, and the passes it grows
/// by a second (lying down: about 3 degrees a frame at 120 frames a second, all of it in 0.25 s).
/// Brought in all at once, a rise's squat cut back down threw the thigh 15 units in a frame
/// (refused, then a 29 m/s snap: the trench replay).
pub(crate) const LIMIT_PASSES: usize = 180;
pub(crate) const LIMIT_RAMP: f32 = 720.0;
/// A light hit's stagger (under the knockdown threshold) is held on New Physics' standing balance
/// at least this long (s) before it may go back to the game's animation, upright and still, and at
/// most this long (s) before it goes back upright all the same (not upright: down on Delirium).
pub(crate) const STAGGER_LEAST: f32 = 1.0;
pub(crate) const STAGGER_MOST: f32 = 6.0;

/// Whether New Physics' own bodies are the real body of an actor whose Delirium slot is `slot`
/// (its solver's loads against walls and the floor are then the body's): no Delirium, or
/// Delirium's parts on New Physics' rise (its pose drawn) for [`NP_SETTLED_IN`] already. While
/// Delirium's own reactions have it (hit, lying, dead), New Physics' bodies are only put on
/// Delirium's pose each frame, and its solver's loads (shoving those bodies out of what they were
/// put into) say nothing of the body: the trench replay read 160 kN on a wall from that; and so
/// for the first frames of a rise, its bodies pushed out of its own floors from Delirium's pose
/// (34 kN on the spine in a frame, the body drawn at under 3 m/s).
pub(crate) fn np_is_the_body(slot: &Slot) -> bool {
    slot.body.as_ref().map_or(true, |d| {
        !d.dead() && d.own.follows() && d.np_for > NP_SETTLED_IN
    })
}

/// A pose of Delirium's that moved more than this in a frame (game units) is not drawn.
pub(crate) const POSE_JUMP: f32 = 25.0;

/// What is drawn of a body Delirium has had turns each joint at most this fast (rad/s) and moves
/// its root at most this fast (m/s) ([`continuous`]).
pub(crate) const TURN_RATE: f32 = 12.0;
pub(crate) const ROOT_RATE: f32 = 6.0;

/// The pose to draw this frame, from the one drawn last (`last`) toward the one wanted (`want`,
/// both each body's node's world, game units), `dt` seconds on: made again through FNV's own
/// skeleton, each bone at its own length under its parent (the bones New Physics has no body for,
/// the clavicles, the pelvis, the neck, at their bind turn under theirs), each joint turned
/// toward the wanted turn at most [`TURN_RATE`], the root moved at most [`ROOT_RATE`]. So what
/// is drawn never jumps (a refusal, a retake, the skeleton's limits brought in, Delirium's own
/// glitches: 23 to 62 units in a frame in Patrick's PC run on 5f52c0e) and never stretches (the
/// upper arms 24 to 30% off their bones' length there: Delirium's clavicles move them where
/// FNV's, never drawn by New Physics, are not).
pub(crate) fn continuous(last: &[(M3, V3)], want: &[(M3, V3)], dt: f32) -> Vec<(M3, V3)> {
    let m = crate::new_physics::model();
    let s = skel();
    if last.len() != want.len() || want.len() <= m.root || dt <= 0.0 {
        return want.to_vec();
    }
    let o = want[m.root].1;
    let (wl, ww) = (skeleton_from_bodies(last, o), skeleton_from_bodies(want, o));
    let n = s.names.len();
    let root = s.names.iter().position(|x| *x == m.bodies[m.root].node);
    // The root body's bone and every bone above it: the chain's start, not made under a parent.
    let mut above = vec![false; n];
    let mut b = root;
    while let Some(i) = b {
        above[i] = true;
        b = s.parents[i];
    }
    let toward = |a: Quat, b: Quat, most: f32| {
        let b = if a.dot(b) < 0.0 { -b } else { b };
        let angle = a.angle_between(b);
        if angle <= most || angle < 1e-6 {
            b
        } else {
            a.slerp(b, most / angle).normalize()
        }
    };
    let turn_most = TURN_RATE * dt;
    let move_most = ROOT_RATE * dt;
    let mut out: Vec<Transform> = Vec::with_capacity(n);
    for b in 0..n {
        let t = match s.parents[b] {
            Some(p) if !above[b] => {
                let local_last = wl[p].rotation.inverse() * wl[b].rotation;
                let local_want = ww[p].rotation.inverse() * ww[b].rotation;
                let q = toward(local_last, local_want, turn_most);
                let pw = out[p];
                Transform::from_translation(pw.translation + pw.rotation * s.local_t[b])
                    .with_rotation((pw.rotation * q).normalize())
            }
            _ => {
                let d = ww[b].translation - wl[b].translation;
                let l = d.length();
                let at = if l > move_most {
                    wl[b].translation + d * (move_most / l)
                } else {
                    ww[b].translation
                };
                Transform::from_translation(at).with_rotation(toward(
                    wl[b].rotation,
                    ww[b].rotation,
                    turn_most,
                ))
            }
        };
        out.push(t);
    }
    m.bodies
        .iter()
        .enumerate()
        .map(|(i, body)| {
            s.names
                .iter()
                .position(|x| *x == body.node)
                .map_or(want[i], |k| from_tf(&out[k], o))
        })
        .collect()
}

/// Back on Delirium's own body from New Physics' rise pose, the difference eased out over this (s).
pub(crate) const EASE_BACK: f32 = 0.2;

/// Each body's offset from `from` to `to` (the turn `to·from⁻¹`, the move).
fn offsets(from: &[(M3, V3)], to: &[(M3, V3)]) -> Vec<(M3, V3)> {
    use ragdoll_core::flinch::{mul, sub, transpose};
    from.iter()
        .zip(to)
        .map(|(a, b)| (mul(&b.0, &transpose(&a.0)), sub(b.1, a.1)))
        .collect()
}

/// `pose` with `left` (0 to 1) of each body's offset `offs` put on it.
fn eased(pose: &[(M3, V3)], offs: &[(M3, V3)], left: f32) -> Vec<(M3, V3)> {
    use ragdoll_core::active::{m_from_q, qfrom_m};
    use ragdoll_core::flinch::{add, mul, scale};
    let k = left.clamp(0.0, 1.0);
    pose.iter()
        .zip(offs)
        .map(|((r, t), (dr, dt))| {
            let q = qfrom_m(dr);
            let s = if q[0] < 0.0 { -1.0 } else { 1.0 };
            let part = [
                1.0 + (s * q[0] - 1.0) * k,
                s * q[1] * k,
                s * q[2] * k,
                s * q[3] * k,
            ];
            let n = part.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
            let part = part.map(|x| x / n);
            (mul(&m_from_q(part), r), add(*t, scale(*dt, k)))
        })
        .collect()
}

/// New Physics' bodies this long the body (s) before their loads are its.
pub(crate) const NP_SETTLED_IN: f32 = 0.15;

/// What a Delirium-owned body is doing (Patrick's plan: Delirium has a human's body for the whole
/// fall, never the game's ragdoll). New Physics' downed controller decides the settle and the rise
/// (its timings: settled before it rises, 3 s on a slope, then up to 5 s to get up, the 15-unit
/// cap once upright) and gives the pose; the body is Delirium's throughout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Own {
    /// Hit: Delirium's own reactions (its shot, its blast, its fall).
    Hit,
    /// Down on Delirium: lying as Delirium has it (its fall, then its own relax); New Physics'
    /// downed controller reads the body as it lies, its settle timed on it.
    Down,
    /// Rising on Delirium: its muscles on New Physics' rise (the arms pushing off through planted
    /// hands), the pelvis helped toward New Physics'.
    Rise,
    /// New Physics stood: its muscles on New Physics' standing pose until it is upright and still
    /// (seconds since it stood, seconds upright and still).
    Up { since: f32, still: f32 },
    /// A light hit standing (under the knockdown threshold): its muscles on New Physics' standing
    /// balance (its stagger, its catching steps) until it is upright and still (seconds since the
    /// hit, seconds upright and still); down on Delirium if New Physics' balance goes down, and
    /// Delirium's own reaction for a hit past the threshold. On its own balance, Delirium's stagger
    /// went down 1.6 to 2.2 s after every 9mm of Patrick's PC run on 8a9b4a4 (the stagger aid that
    /// pushed its body back over its feet or not); New Physics' balance stepped and stood.
    Stagger { since: f32, still: f32 },
}

impl Own {
    /// Its muscles on New Physics' pose (rising, standing, staggering): that pose is drawn.
    pub(crate) fn follows(self) -> bool {
        matches!(self, Own::Rise | Own::Up { .. } | Own::Stagger { .. })
    }
}

/// A change of [`Own`]'s, each said in the log.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Change {
    /// New Physics' rise began: Delirium's muscles on it.
    Rising,
    /// The rise stopped (given up, or waiting): down on Delirium again, relaxed.
    BackDown,
    /// New Physics stands: Delirium held on its standing pose.
    Standing,
    /// Upright and still: back to the game's animation.
    Upright,
    /// Not upright [`UP_MOST`] seconds after New Physics stood: New Physics has it.
    Stuck,
    /// Staggering, New Physics' balance went down: down on Delirium, its own fall from there.
    Fell,
}

/// Still enough to hand back (m/s, the centre of mass), for [`UP_STILL_FOR`] seconds.
pub(crate) const UP_STILL: f32 = 0.15;
pub(crate) const UP_STILL_FOR: f32 = 0.3;
/// The longest Delirium's body may take to come upright after New Physics stood (s).
pub(crate) const UP_MOST: f32 = 3.0;

/// The next [`Own`] of a body on Delirium's muscles or lying (not [`Own::Hit`]): from whether New
/// Physics' controller has it downed and rising, and whether Delirium's body is upright and how
/// fast it moves (m/s), `dt` seconds on.
pub(crate) fn next_own(
    own: Own,
    np_down: bool,
    np_rise: bool,
    upright: bool,
    speed: f32,
    dt: f32,
) -> (Own, Option<Change>) {
    let stood = Own::Up {
        since: 0.0,
        still: 0.0,
    };
    match own {
        Own::Hit => (own, None),
        Own::Down if !np_down => (stood, Some(Change::Standing)),
        Own::Down if np_rise => (Own::Rise, Some(Change::Rising)),
        Own::Down => (own, None),
        Own::Rise if !np_down => (stood, Some(Change::Standing)),
        Own::Rise if !np_rise => (Own::Down, Some(Change::BackDown)),
        Own::Rise => (own, None),
        Own::Stagger { .. } if np_down => (Own::Down, Some(Change::Fell)),
        Own::Stagger { since, still } => {
            let since = since + dt;
            let still = if upright && speed < UP_STILL {
                still + dt
            } else {
                0.0
            };
            let next = Own::Stagger { since, still };
            if still >= UP_STILL_FOR && since >= STAGGER_LEAST || since >= STAGGER_MOST && upright {
                (next, Some(Change::Upright))
            } else if since >= STAGGER_MOST {
                (next, Some(Change::Stuck))
            } else {
                (next, None)
            }
        }
        Own::Up { .. } if np_down && np_rise => (Own::Rise, Some(Change::Rising)),
        Own::Up { .. } if np_down => (Own::Down, Some(Change::BackDown)),
        Own::Up { since, still } => {
            let since = since + dt;
            let still = if upright && speed < UP_STILL {
                still + dt
            } else {
                0.0
            };
            let next = Own::Up { since, still };
            if still >= UP_STILL_FOR {
                (next, Some(Change::Upright))
            } else if since >= UP_MOST {
                (next, Some(Change::Stuck))
            } else {
                (next, None)
            }
        }
    }
}

/// FNV's whole skeleton's worlds (metres from `origin`, game units) for New Physics' bodies at
/// `bodies` (each body's node's world, game units): each body's node where its body is, every
/// other bone (the pelvis, the clavicles, the neck) under its parent as in the bind pose.
// Each bone reads its own world and its parent's, built in order.
#[allow(clippy::needless_range_loop)]
pub(crate) fn skeleton_from_bodies(bodies: &[(M3, V3)], origin: V3) -> Vec<Transform> {
    let s = skel();
    let m = crate::new_physics::model();
    let n = s.names.len();
    let mut w: Vec<Option<Transform>> = vec![None; n];
    for (b, def) in m.bodies.iter().enumerate() {
        if let (Some(i), Some((r, t))) =
            (s.names.iter().position(|x| *x == def.node), bodies.get(b))
        {
            w[i] = Some(to_tf(r, *t, origin));
        }
    }
    let mut out = Vec::with_capacity(n);
    for b in 0..n {
        let t = match (w[b], s.parents[b]) {
            (Some(t), _) => t,
            (None, Some(p)) => {
                let pw: Transform = out[p];
                let local = s.rest[p].rotation.inverse() * s.rest[b].rotation;
                Transform::from_translation(pw.translation + pw.rotation * s.local_t[b])
                    .with_rotation((pw.rotation * local).normalize())
            }
            (None, None) => s.rest[b],
        };
        out.push(t);
    }
    out
}

/// Delirium's parts' worlds (metres from `origin`) for New Physics' bodies at `bodies`: the pose
/// Delirium's muscles follow.
pub(crate) fn parts_from_bodies(rig: &Rig, bodies: &[(M3, V3)], origin: V3) -> Vec<Transform> {
    rig.parts_at(&skeleton_from_bodies(bodies, origin))
}

/// What a frame of Delirium came to.
pub(crate) enum Step {
    /// Delirium has it: New Physics' bodies' worlds (game units).
    Drawn(Vec<(M3, V3)>),
    /// It lies settled, or stood balanced again a while: New Physics goes on from its pose.
    Done(Vec<(M3, V3)>, &'static str),
}

/// The part of Delirium's body a New Physics body's hit lands on.
pub(crate) fn part_of_body(body: usize) -> usize {
    let m = crate::new_physics::model();
    // Up New Physics' bodies to one whose bone follows a part (each of its bodies' does).
    let mut i = body;
    for _ in 0..m.bodies.len() {
        if let Some(p) = m
            .bodies
            .get(i)
            .and_then(|b| delirium_bridge::rig::fnv_part(&b.node))
        {
            return p;
        }
        match m.joints.iter().find(|j| j.child == i) {
            Some(j) => i = j.parent,
            None => break,
        }
    }
    0
}

impl Dl {
    /// The actor's Delirium, from its bones' worlds (`bone`: an FNV bone's world by name, game
    /// units), each part moving as `velocity` has it (m/s), at `origin`.
    pub(crate) fn take(
        bone: &dyn Fn(&str) -> Option<(M3, V3)>,
        velocity: &dyn Fn(usize) -> [f32; 3],
        origin: V3,
        profile: &delirium_bridge::profile::Profile,
    ) -> Result<Dl, String> {
        let rig = rig().ok_or("Delirium isn't running")?;
        let s = skel();
        let mut worlds = s.rest.clone();
        for (i, n) in s.names.iter().enumerate() {
            if delirium_bridge::rig::fnv_part(n).is_some() {
                let (r, t) = bone(n).ok_or_else(|| format!("no {n} in the actor's tree"))?;
                worlds[i] = to_tf(&r, t, origin);
            }
        }
        let parts = rig.parts_at(&worlds);
        let states: Vec<PartState> = parts
            .iter()
            .enumerate()
            .map(|(k, &frame)| PartState {
                frame,
                velocity: Vec3::from_array(velocity(k)),
                spin: Vec3::ZERO,
            })
            .collect();
        let agent = make_body(&states, profile)?;
        Ok(Dl {
            agent,
            rig,
            origin,
            frames: 0,
            balanced_for: 0.0,
            standing: 0.0,
            last: Vec::new(),
            pending: 0.0,
            steps: (Vec::new(), Vec::new()),
            corpse: None,
            killed: false,
            dying: None,
            own: Own::Hit,
            ground: None,
            glitch: 0,
            limited: 0.0,
            limit_in: 0.0,
            np_for: 0.0,
            ease: None,
            target: None,
            guide: None,
            age: 0.0,
            worst: "",
        })
    }

    /// Dead: its muscles off (Delirium's corpse), its ragdoll kept on the pose drawn when the
    /// game's death would turn it loose. Answers whether it is held (an actor with no ragdoll
    /// controller is not).
    pub(crate) fn die(&mut self, e: &engine::prelude::Engine, actor: u32) -> Result<bool, String> {
        if self.corpse.is_none() {
            self.corpse = CorpseHold::of(e, actor);
            if self.corpse.is_none() {
                return Ok(false);
            }
            // Mid-rise: its wait first, the limbs brought in (`frame_for`); else limp at once.
            if matches!(self.own, Own::Rise | Own::Up { .. }) {
                self.dying = Some(0);
            } else {
                self.kill()?;
            }
        }
        Ok(true)
    }

    /// Its muscles off for good.
    fn kill(&mut self) -> Result<(), String> {
        self.dying = None;
        self.killed = true;
        self.agent.kill()
    }

    /// Its drive for `own`: down again after its muscles followed (relaxed, as Delirium's own
    /// relax has it, nothing followed), or its muscles on New Physics' pose (rising, standing).
    /// Down from a hit leaves Delirium's own fall going on.
    fn drive(&mut self, own: Own) -> Result<(), String> {
        match own {
            // Its own reactions again (from a pose followed: a hit past the threshold staggering).
            Own::Hit => {
                self.agent.follow(&[])?;
                self.agent.rise(false, 0.0)
            }
            Own::Down => {
                self.agent.follow(&[])?;
                self.agent.rise(false, 0.0)?;
                self.agent.relax()
            }
            Own::Rise | Own::Up { .. } | Own::Stagger { .. } => self.agent.rise(true, RISE_HELP),
        }
    }

    /// Its muscles on New Physics' bodies at `bodies` (game units).
    fn follow(&mut self, rig: &Rig, bodies: &[(M3, V3)]) -> Result<(), String> {
        self.agent
            .follow(&parts_from_bodies(rig, bodies, self.origin))
    }

    /// Whether its agent keeps its balance on its feet now.
    pub(crate) fn agent_balancing(&self) -> bool {
        self.agent.balancing()
    }

    /// Whether it is a corpse Delirium keeps.
    pub(crate) fn dead(&self) -> bool {
        self.corpse.is_some()
    }

    /// A shot on New Physics' body `body` at `point` (game units) along `dir`.
    pub(crate) fn shot(
        &mut self,
        body: usize,
        point: V3,
        dir: V3,
        health: f32,
    ) -> Result<(), String> {
        let at = Vec3::new(
            (point[0] - self.origin[0]) / UNITS,
            (point[1] - self.origin[1]) / UNITS,
            (point[2] - self.origin[2]) / UNITS,
        );
        let d = Vec3::from_array(dir).normalize_or_zero();
        // `dir` is the hit's impulse (N s): its size sets the shot's force.
        let p = ragdoll_core::flinch::length(dir);
        self.agent
            .shot(part_of_body(body), at, d, health, shot_force(p))
    }

    /// A blast at `centre` (game units), reaching `radius` (game units), `speed` m/s at its centre.
    pub(crate) fn blast(&mut self, centre: V3, radius: f32, speed: f32) -> Result<(), String> {
        let at = Vec3::new(
            (centre[0] - self.origin[0]) / UNITS,
            (centre[1] - self.origin[1]) / UNITS,
            (centre[2] - self.origin[2]) / UNITS,
        );
        self.agent.blast(at, radius / UNITS, speed)
    }

    /// One frame: the world set to `mesh`, the agent stepped by `dt`, its pose as New Physics'
    /// bodies; done once it lies settled half a second (or stood balanced two seconds).
    pub(crate) fn frame(
        &mut self,
        mesh: &delirium_bridge::world::Mesh,
        dt: f32,
    ) -> Result<Step, String> {
        let rig = self.rig;
        use delirium_bridge::agent::STEP;
        self.agent.set_mesh(mesh)?;
        if self.steps.1.is_empty() {
            let now = self.agent.parts()?;
            self.steps = (now.clone(), now);
        }
        // Whole steps of Delirium's own (at most four a frame: a long frame is not caught up).
        self.pending += dt.clamp(0.0, 4.0 * STEP);
        while self.pending >= STEP - 1e-6 {
            // Rising or held standing: its parts on New Physics' pose (moving as it moves).
            if let Some((g, v)) = self.guide.as_ref() {
                if g.len() == 21 {
                    for (k, frame) in g.iter().enumerate() {
                        self.agent.set_part(
                            k,
                            &PartState {
                                frame: *frame,
                                velocity: v.get(k).copied().unwrap_or(Vec3::ZERO),
                                spin: Vec3::ZERO,
                            },
                        )?;
                    }
                }
            }
            self.agent.step(STEP)?;
            self.pending -= STEP;
            let now = self.agent.parts()?;
            self.steps.0 = std::mem::replace(&mut self.steps.1, now);
        }
        self.frames += 1;
        if self.steps.1.len() != 21 || self.steps.0.len() != 21 {
            return Err(format!("{} parts back", self.steps.1.len()));
        }
        let parts = between(
            &self.steps.0,
            &self.steps.1,
            (self.pending / STEP).clamp(0.0, 1.0),
        );
        let drawn = bodies_from_parts(rig, &parts, self.origin);
        // Once down, New Physics' downed controller says when it is done (`next_own`).
        if self.own != Own::Hit {
            return Ok(Step::Drawn(drawn));
        }
        if self.agent.should_get_up(0.5) {
            return Ok(Step::Done(drawn, "it lies settled"));
        }
        if self.agent.balancing() {
            self.balanced_for += dt;
        } else {
            self.balanced_for = 0.0;
        }
        if self.balanced_for >= 2.0 {
            return Ok(Step::Done(drawn, "it stands balanced again"));
        }
        Ok(Step::Drawn(drawn))
    }
}

/// An actor's base form (`TESObjectREFR::baseForm`), its form type (`+4`), an NPC's weight
/// slider (`TESNPC::fWeight`), the actor-value owner and its base and current value slots, health.
const BASE_FORM: u32 = 0x20;
const FORM_TYPE: u32 = 0x04;
const NPC_FORM: u8 = 0x2a;
const NPC_WEIGHT: u32 = 0x1f8;
const AV_OWNER: u32 = engine::actor_values::OWNER;
const AV_BASE: u32 = 0x4;
const AV_HEALTH: u32 = 0x10;

/// What FNV knows of `actor` for its profile: NPC or creature, its weight slider. Its race is a
/// human's (ghouls share the human skeleton) and its body armor none until those are read for it:
/// every profile row is neutral for now, so neither changes a setting yet.
pub(crate) fn facts_of(
    e: &mut engine::prelude::Engine,
    actor: u32,
) -> delirium_bridge::profile::Facts {
    use delirium_bridge::profile::{Actor, Armor, Facts, Race};
    let base = e.mem.u32(actor + BASE_FORM);
    let npc = base != 0 && e.mem.u8(base + FORM_TYPE) == NPC_FORM;
    let weight = if npc {
        let w = e.mem.f32(base + NPC_WEIGHT);
        if w.is_finite() {
            // The slider as the form keeps it: 0 to 1 (its clear-data sets 0.5).
            (w.clamp(0.0, 1.0) * 100.0).round() as u8
        } else {
            50
        }
    } else {
        50
    };
    Facts {
        race: Race::Human,
        actor: if npc { Actor::Npc } else { Actor::Creature },
        armor: Armor::None,
        weight,
    }
}

/// The share of its health `actor` has left (0 to 1; 1 when unknown): Delirium's shot task takes
/// it (a lot left staggers, little drops it).
pub(crate) fn health_left(e: &mut engine::prelude::Engine, actor: u32) -> f32 {
    // No actor-value owner's table (a half-made actor): unknown.
    let table = actor.wrapping_add(AV_OWNER);
    let vtable = if e.mem.is_mapped(table) {
        e.mem.u32(table)
    } else {
        0
    };
    let slot = vtable.wrapping_add(AV_BASE);
    if vtable == 0 || !e.mem.is_mapped(slot) || e.mem.u32(slot) == 0 {
        return 1.0;
    }
    let now = engine::actor_values::current(e, actor, AV_HEALTH);
    let most = e
        .vcall(
            actor.wrapping_add(AV_OWNER),
            AV_BASE,
            &engine::prelude::args![AV_HEALTH],
        )
        .f64() as f32;
    match now {
        Some(now) if most.is_finite() && most > 0.0 => (now / most).clamp(0.0, 1.0),
        _ => 1.0,
    }
}

/// The body's posture and balance as Delirium's pose has it (New Physics' bodies' worlds, game
/// units): the pelvis over the floor, the torso's lean from upright (pelvis to head), the body's
/// speed, and the capture point (the centre of mass carried by its speed, `x + v sqrt(h/g)`)
/// outside the feet (metres; under 0 inside them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Posture {
    pub pelvis_up: f32,
    pub lean: f32,
    pub speed: f32,
    pub outside: f32,
    /// The centre of mass and its velocity (metres, m/s), and between the feet (metres).
    pub com: V3,
    pub v: V3,
    pub feet: V3,
}

impl Posture {
    /// Down: the pelvis under 55% of its standing height, or the torso more than 60 degrees from
    /// upright (fallen over, lying, seated on the ground).
    pub(crate) fn downed(&self, standing: f32) -> bool {
        self.pelvis_up < 0.55 * standing || self.lean > 60.0
    }

    /// Upright: the pelvis at 85% of its standing height or more, the torso within 20 degrees of
    /// upright.
    pub(crate) fn upright(&self, standing: f32) -> bool {
        self.pelvis_up >= 0.85 * standing && self.lean < 20.0
    }
}

/// [`Posture`] of `drawn` over `floor` (game units), moving from `before` over `dt` seconds.
pub(crate) fn posture(drawn: &[(M3, V3)], before: &[(M3, V3)], floor: f32, dt: f32) -> Posture {
    use ragdoll_core::flinch::{apply_m, length, scale, sub};
    let m = crate::new_physics::model();
    let at = |name: &str| m.body(name).and_then(|b| drawn.get(b)).map(|w| w.1);
    let pelvis = drawn.get(m.root).map_or([0.0; 3], |w| w.1);
    let head = at("Bip01 Head").unwrap_or(pelvis);
    let up = sub(head, pelvis);
    let lean = if length(up) > 1e-3 {
        (up[2] / length(up)).clamp(-1.0, 1.0).acos().to_degrees()
    } else {
        0.0
    };
    // The centre of mass and its speed (metres, m/s).
    let centre = |worlds: &[(M3, V3)]| {
        let mut c = [0.0f32; 3];
        let mut total = 0.0;
        for (b, def) in m.bodies.iter().enumerate() {
            if let Some((r, t)) = worlds.get(b) {
                let p = ragdoll_core::flinch::add(*t, apply_m(r, scale(def.com, UNITS)));
                c = ragdoll_core::flinch::add(c, scale(p, def.mass));
                total += def.mass;
            }
        }
        scale(c, 1.0 / (total.max(1e-6) * UNITS))
    };
    let c = centre(drawn);
    let v = if dt > 0.0 && before.len() == drawn.len() {
        scale(sub(c, centre(before)), 1.0 / dt)
    } else {
        [0.0; 3]
    };
    let height = (c[2] - floor / UNITS).max(0.05);
    let capture = ragdoll_core::flinch::add(
        c,
        scale(v, (height / ragdoll_core::np_body::GRAVITY).sqrt()),
    );
    let feet: Vec<V3> = ["Bip01 L Foot", "Bip01 R Foot"]
        .iter()
        .filter_map(|n| at(n))
        .map(|f| scale(f, 1.0 / UNITS))
        .collect();
    // Outside the feet: from the segment between them, less a foot's half width.
    let outside = match feet.as_slice() {
        [a, b] => {
            let ab = [b[0] - a[0], b[1] - a[1]];
            let ap = [capture[0] - a[0], capture[1] - a[1]];
            let l2 = ab[0] * ab[0] + ab[1] * ab[1];
            let t = if l2 > 1e-9 {
                ((ap[0] * ab[0] + ap[1] * ab[1]) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = [ap[0] - t * ab[0], ap[1] - t * ab[1]];
            (d[0] * d[0] + d[1] * d[1]).sqrt() - 0.1
        }
        _ => 0.0,
    };
    let between_feet = match feet.as_slice() {
        [a, b] => scale(ragdoll_core::flinch::add(*a, *b), 0.5),
        _ => c,
    };
    Posture {
        pelvis_up: pelvis[2] - floor,
        lean,
        speed: length([v[0], v[1], v[2]]),
        outside,
        com: c,
        v,
        feet: between_feet,
    }
}

/// New Physics' bodies' velocities (m/s, rad/s) from two of Delirium's poses `dt` apart.
pub(crate) fn velocities(drawn: &[(M3, V3)], before: &[(M3, V3)], dt: f32) -> Vec<(V3, V3)> {
    drawn
        .iter()
        .enumerate()
        .map(|(b, (r, t))| match before.get(b) {
            Some((r0, t0)) if dt > 0.0 => {
                let v = ragdoll_core::flinch::scale(
                    ragdoll_core::flinch::sub(*t, *t0),
                    1.0 / (dt * UNITS),
                );
                let q = ragdoll_core::active::qfrom_m(&ragdoll_core::pose::ortho(r));
                let q0 = ragdoll_core::active::qfrom_m(&ragdoll_core::pose::ortho(r0));
                let turn = ragdoll_core::active::qmul(q, ragdoll_core::active::qconj(q0));
                (
                    v,
                    ragdoll_core::flinch::scale(ragdoll_core::active::qlog(turn), 1.0 / dt),
                )
            }
            _ => ([0.0; 3], [0.0; 3]),
        })
        .collect()
}

/// New Physics' bodies put on `drawn` (each body's node's world, game units) moving as `moving`
/// (m/s, rad/s): in New Physics' own frame, about its origin (`Np::origin`), as its frame keeps
/// them. Patrick's 22:38 log: put there in the world's frame, New Physics found the body thousands
/// of units off when it took it back, read a fall and handed that pose to the game's ragdoll,
/// which flung it out of the world (the pelvis 1628 units over the navmesh).
pub(crate) fn put_on(np: &mut ragdoll_core::np::Np, drawn: &[(M3, V3)], moving: &[(V3, V3)]) {
    let m = crate::new_physics::model();
    let o = np.origin;
    let local: Vec<(M3, V3)> = drawn
        .iter()
        .map(|(r, t)| (*r, ragdoll_core::flinch::sub(*t, o)))
        .collect();
    np.states = m.states_from_nodes(&local);
    for (st, (v, w)) in np.states.iter_mut().zip(moving) {
        st.v = *v;
        st.w = *w;
    }
}

/// The impulse (N s) Delirium's shot force is scaled from: a hit this strong gets Delirium's own
/// force ([`delirium_bridge::agent::SHOT_FORCE`]), a lighter one less, a heavier more (a 9mm's
/// 25 N s a flinch or a small step, not GTA IV's pistol lurch: Patrick's 22:38 clip).
pub(crate) const SHOT_IMPULSE_REF: f32 = 60.0;

/// Delirium's shot force for a hit of `impulse` N s: [`SHOT_IMPULSE_REF`]'s share of its own,
/// from a tenth to three times it.
pub(crate) fn shot_force(impulse: f32) -> f32 {
    let k = if impulse.is_finite() {
        impulse / SHOT_IMPULSE_REF
    } else {
        1.0
    };
    delirium_bridge::agent::SHOT_FORCE * k.clamp(0.1, 3.0)
}

/// The parts `t` of the way from `a` to `b` (each part's place and turn).
pub(crate) fn between(a: &[Transform], b: &[Transform], t: f32) -> Vec<Transform> {
    a.iter()
        .zip(b)
        .map(|(x, y)| {
            Transform::from_translation(x.translation.lerp(y.translation, t))
                .with_rotation(x.rotation.slerp(y.rotation, t).normalize())
        })
        .collect()
}

/// New Physics' bodies at `bodies` (each body's node's world, game units) brought inside the
/// skeleton's limits (its data's, as New Physics brings a downed body's), and how far out the
/// farthest joint was before (radians). Under [`LIMIT_HOLD`] the bodies as they are.
pub(crate) fn into_limits(
    bodies: &[(M3, V3)],
    passes: usize,
) -> (Vec<(M3, V3)>, f32, &'static str) {
    let m = crate::new_physics::model();
    let Some(o) = bodies.get(m.root).map(|b| b.1) else {
        return (bodies.to_vec(), 0.0, "");
    };
    let local: Vec<(M3, V3)> = bodies
        .iter()
        .map(|(r, t)| (*r, ragdoll_core::flinch::sub(*t, o)))
        .collect();
    let mut st = m.states_from_nodes(&local);
    let (worst, out) = m
        .data_limits_out(&st)
        .into_iter()
        .enumerate()
        .filter(|x| x.1.is_finite())
        .fold((0, 0.0f32), |a, b| if b.1 > a.1 { b } else { a });
    let name = m
        .joints
        .get(worst)
        .and_then(|j| m.bodies.get(j.child))
        .map_or("", |b| b.node.as_str());
    if out <= LIMIT_HOLD {
        return (bodies.to_vec(), out, name);
    }
    // Each pass brings a joint in a small step.
    m.into_data_limits(&mut st, passes);
    let inside = m
        .node_worlds(&st)
        .into_iter()
        .map(|(r, t)| (r, ragdoll_core::flinch::add(t, o)))
        .collect();
    (inside, out, name)
}

/// The height of the floor `floors` has nearest (flat) to `at` (game units) at `at`, else
/// `floor`: where a ground cast there starts from.
pub(crate) fn floor_guess(floors: &[Plane], at: [f32; 2], floor: f32) -> f32 {
    let p = [at[0] / UNITS, at[1] / UNITS];
    floors
        .iter()
        .filter(|pl| pl.n[2] > 0.3)
        .min_by(|a, b| {
            let d = |pl: &Plane| (pl.p[0] - p[0]).powi(2) + (pl.p[1] - p[1]).powi(2);
            d(a).total_cmp(&d(b))
        })
        .map_or(floor, |pl| {
            let z = pl.p[2] - (pl.n[0] * (p[0] - pl.p[0]) + pl.n[1] * (p[1] - pl.p[1])) / pl.n[2];
            z * UNITS
        })
}

/// The ground around `centre` (game units) as a height field from `origin` (metres): each of
/// [`GROUND_N`] by [`GROUND_N`] points [`GROUND_SPACING`] apart cast down by `cast` (from a
/// height `guess` gives it there, so a slope's or a rock's top is met and a table's top passed
/// under), `guess`'s own height where the cast meets nothing.
pub(crate) fn ground_mesh(
    centre: V3,
    origin: V3,
    guess: &dyn Fn([f32; 2]) -> f32,
    cast: &mut dyn FnMut(V3) -> Option<f32>,
) -> delirium_bridge::world::Mesh {
    let n = GROUND_N;
    let half = GROUND_SPACING * (n - 1) as f32 / 2.0;
    let corner = [centre[0] / UNITS - half, centre[1] / UNITS - half];
    let mut heights = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let x = (corner[0] + i as f32 * GROUND_SPACING) * UNITS;
            let y = (corner[1] + j as f32 * GROUND_SPACING) * UNITS;
            // From above the body (a trench's bank higher than the floor guessed: Patrick's 23:16
            // clip, heels in the dirt), else from over the guess, else the guess.
            let z0 = guess([x, y]);
            let high = cast([x, y, centre[2].max(z0) + 100.0]);
            let z = high.or_else(|| cast([x, y, z0 + 40.0])).unwrap_or(z0);
            heights.push((z - origin[2]) / UNITS);
        }
    }
    delirium_bridge::world::heightfield(
        [corner[0] - origin[0] / UNITS, corner[1] - origin[1] / UNITS],
        GROUND_SPACING,
        n,
        &heights,
    )
}

/// Why a pose of Delirium's is not to be drawn (none: it may be): a body that moved more than
/// [`POSE_JUMP`] units since the last frame, one more than 3 m from the pelvis (a body flung apart), or the
/// pelvis more than 10 m from where the take began (`origin`). Patrick's 22:38 clip: two bodies
/// vanished a few frames after the take, as if flung out of the world.
pub(crate) fn pose_wrong(
    drawn: &[(M3, V3)],
    last: &[(M3, V3)],
    origin: V3,
    dt: f32,
) -> Option<String> {
    // A frame's move: 25 units, or 20 m/s over a long frame. Never the self-test's 15 units
    // (`crate::physics_checks::JUMP_MOST`): at 100 frames a second Delirium's own limbs move that
    // far in a fall, and each refusal and retake from New Physics' pose was itself a jump
    // (Patrick's PC run on 5f52c0e: refused three times a scenario, then New Physics kept it).
    // What is drawn is kept smooth by [`continuous`].
    let most = (20.0 * UNITS * dt).max(POSE_JUMP);
    use ragdoll_core::flinch::{length, sub};
    let m = crate::new_physics::model();
    let pelvis = drawn.get(m.root)?.1;
    if length(sub(pelvis, origin)) > 10.0 * UNITS {
        return Some(format!(
            "the pelvis {:.0} units from the take",
            length(sub(pelvis, origin))
        ));
    }
    for (b, (_, t)) in drawn.iter().enumerate() {
        let apart = length(sub(*t, pelvis));
        if apart > 3.0 * UNITS {
            return Some(format!(
                "{} {apart:.0} units from the pelvis",
                m.bodies[b].node
            ));
        }
        if let Some((_, t0)) = last.get(b) {
            let moved = length(sub(*t, *t0));
            if moved > most {
                return Some(format!(
                    "{} moved {moved:.0} units in a frame",
                    m.bodies[b].node
                ));
            }
        }
    }
    None
}

/// A hit waiting for the actor's Delirium frame: a shot (New Physics' body, its point in game
/// units, its impulse N s) or a blast (its centre and radius in game units, its speed m/s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DlHit {
    Shot(usize, V3, V3),
    Blast(V3, f32, f32),
}

/// A hit's pushes as one Delirium shot (its strongest: where the round struck), queued where
/// Delirium runs. Every pellet and every round of a burst is its own: nothing capped.
pub(crate) fn queue_shot(queue: &mut Vec<DlHit>, pushes: &[(usize, V3, V3)]) {
    if !running() {
        return;
    }
    let strongest = pushes.iter().max_by(|a, b| {
        ragdoll_core::flinch::length(a.2).total_cmp(&ragdoll_core::flinch::length(b.2))
    });
    if let Some(&(b, point, impulse)) = strongest {
        queue.push(DlHit::Shot(b, point, impulse));
    }
}

/// A blast queued for Delirium in place of the shot its pushes just queued.
pub(crate) fn queue_blast(queue: &mut Vec<DlHit>, centre: V3, radius: f32, speed: f32) {
    if !running() {
        return;
    }
    if matches!(queue.last(), Some(DlHit::Shot(..))) {
        queue.pop();
    }
    queue.push(DlHit::Blast(centre, radius, speed));
}

/// How many times a body Delirium refused is taken again from New Physics' pose (each refusal
/// one; a corpse made at a death after New Physics had it too) before New Physics keeps it.
pub(crate) const RETAKES: u32 = 3;

/// An actor's Delirium, where Delirium runs: its body there, the hits queued for it, and who has
/// had it.
#[derive(Default)]
pub(crate) struct Slot {
    pub body: Option<Dl>,
    /// Tried (a take made or refused): taken again only as below.
    pub tried: bool,
    pub queue: Vec<DlHit>,
    /// Left by Delirium while downed (New Physics' downed controller has it), its get-up begun:
    /// said by the host.
    pub down: bool,
    pub rising: bool,
    /// Handed back upright and still: taken again by a new hit.
    pub back: bool,
    /// Refused: taken again at once from New Physics' pose, [`RETAKES`] times at most.
    pub again: bool,
    pub retakes: u32,
    /// Delirium has had it (a human): never the game's ragdoll, whoever has it now.
    pub had: bool,
    /// Its pelvis over the floor standing (game units): from the animation at the first take, kept
    /// through every retake (Patrick's 15:30 log: taken again lying, its lying height of 22 units
    /// became its standing one).
    pub standing: Option<f32>,
    /// The hits' push stacked (m/s, decaying with the game's impact stacking), kept through a
    /// retake.
    pub pain: f32,
    /// Its corpse could not be made on Delirium (not tried again).
    pub corpse_failed: bool,
}

/// The pelvis's standing height (game units) for an actor whose animation has its root body
/// `anim_pelvis` (game units) over `floor`: the animation's, never under 85% of the bind pose's
/// (an actor taken crouched or mid-fall stands at its own height all the same).
pub(crate) fn standing_height(anim_pelvis: f32, floor: f32) -> f32 {
    let s = skel();
    let bind = s
        .names
        .iter()
        .position(|n| n == "Bip01 NonAccum")
        .map_or(0.95, |i| s.rest[i].translation.z)
        * UNITS;
    (anim_pelvis - floor).max(0.85 * bind)
}

/// A hit's push on the body (m/s, the game's hit force applied) stacked onto `pain` after `dt`
/// seconds of its decay (`half_life` s), and whether the stack reaches `topple` (m/s): a knock-down
/// (Delirium's own fall) or a stagger (kept on its feet).
pub(crate) fn stack_hit(
    pain: f32,
    speed: f32,
    dt: f32,
    half_life: f32,
    topple: f32,
) -> (f32, bool) {
    let left = pain * 0.5f32.powf(dt / half_life.max(1e-3));
    let now = left + speed.max(0.0);
    (now, now >= topple)
}

/// FNV's skeleton's worlds (metres from `origin`) where New Physics has its bodies now.
fn np_skeleton(np: &ragdoll_core::np::Np, origin: V3) -> Vec<Transform> {
    let bodies: Vec<(M3, V3)> = crate::new_physics::model()
        .node_worlds(&np.states)
        .iter()
        .map(|(r, t)| (*r, ragdoll_core::flinch::add(*t, np.origin)))
        .collect();
    skeleton_from_bodies(&bodies, origin)
}

/// A Delirium from New Physics' bodies as they are (at `origin`), moving as its root body.
fn take_from_np(
    np: &ragdoll_core::np::Np,
    origin: V3,
    facts: &delirium_bridge::profile::Facts,
) -> Result<Dl, String> {
    let w = np_skeleton(np, origin);
    let s = skel();
    let bone = |name: &str| {
        s.names
            .iter()
            .position(|n| n == name)
            .map(|i| from_tf(&w[i], origin))
    };
    // Each part moving as New Physics' body its bone is on (or the nearest above it) moves.
    let m = crate::new_physics::model();
    let names: Vec<&str> = s.names.iter().map(String::as_str).collect();
    let part_bones = delirium_bridge::rig::part_bones(&names);
    let v_of = |k: usize| -> [f32; 3] {
        let mut b = part_bones.get(k).copied().flatten().or(part_bones[7]);
        let mut v = np.states.get(m.root).map_or([0.0; 3], |st| st.v);
        while let Some(i) = b {
            if let Some(st) = m.body(&s.names[i]).and_then(|bi| np.states.get(bi)) {
                v = st.v;
                break;
            }
            b = s.parents[i];
        }
        capped(v, RETAKE_MOST)
    };
    Dl::take(
        &bone,
        &v_of,
        origin,
        &delirium_bridge::profile::profile(facts),
    )
}

/// `v` no longer than `most` (and nothing that isn't finite).
pub(crate) fn capped(v: V3, most: f32) -> V3 {
    let l = ragdoll_core::flinch::length(v);
    if !l.is_finite() {
        [0.0; 3]
    } else if l > most {
        ragdoll_core::flinch::scale(v, most / l)
    } else {
        v
    }
}

/// Dead while New Physics has a human Delirium could have (refused, or never taken: Patrick's
/// rule, the corpse is Delirium's): a Delirium made at once from New Physics' pose, to be its
/// corpse. Whether it was made (else the game's death as before, said).
pub(crate) fn corpse_from_new_physics(
    e: &mut engine::prelude::Engine,
    (id, actor): (u32, u32),
    np: &ragdoll_core::np::Np,
    slot: &mut Slot,
) -> bool {
    // Retakes used up or not (they keep a living body Delirium flung from being taken again and
    // again), a corpse is Delirium's: after the third refusal it went to the game's ragdoll
    // (Patrick's PC run on 5f52c0e, kill-mid-rise). Only a corpse Delirium could not make is left
    // to the game.
    if slot.body.is_some() || !running() || slot.corpse_failed {
        return false;
    }
    let facts = facts_of(e, actor);
    if !delirium_bridge::profile::takes(&facts) {
        return false;
    }
    let m = crate::new_physics::model();
    let origin = ragdoll_core::flinch::add(
        m.node_worlds(&np.states)
            .get(m.root)
            .map_or([0.0; 3], |w| w.1),
        np.origin,
    );
    match take_from_np(np, origin, &facts) {
        Ok(d) => {
            crate::new_physics::note_event(format!(
                "{id:08X}: [np delirium] taken at its death from New Physics' pose: its corpse is \
                 Delirium's"
            ));
            slot.body = Some(d);
            slot.tried = true;
            slot.had = true;
            true
        }
        Err(why) => {
            slot.corpse_failed = true;
            crate::new_physics::note_event(format!(
                "{id:08X}: [np delirium] its corpse not taken ({why}): the game's death as before"
            ));
            false
        }
    }
}

/// The actor's frame on Delirium, where it runs: its Delirium made at its first frame (its bones
/// read from the animation's snapshot; taken again from New Physics' bodies after a refusal, or
/// on a new hit after it was handed back upright), the queued hits given, the world New Physics
/// found around it set, a step; the drawn bodies are Delirium's, for the whole fall (Patrick's
/// rule: never the game's ragdoll, alive, downed, rising or dead). Hit, its reactions are
/// Delirium's own and New Physics' bodies are kept on its pose; downed, it lies as Delirium has it
/// while New Physics' downed controller reads it and times its settle; rising and standing, its
/// muscles follow New Physics' rise (the arms pushing off through planted hands) until it is
/// upright and still, then the game's animation has it again; dead mid-rise, its wait first, the
/// limbs brought in. None: New Physics' own frame (not taken, refused while hit, handed back).
#[allow(clippy::type_complexity)]
/// Staggering, a hit past the knockdown threshold (stacked): Delirium's own reaction from here,
/// its muscles off New Physics' standing balance.
fn toppled(id: u32, d: &mut Dl, pain: f32, topple: f32) -> Result<(), String> {
    if !matches!(d.own, Own::Stagger { .. }) || d.killed {
        return Ok(());
    }
    d.own = Own::Hit;
    crate::new_physics::note_event(format!(
        "{id:08X}: [np delirium] staggering, the hits past the knockdown threshold ({pain:.2} of \
         {topple:.2} m/s stacked): Delirium's own reaction from here, a fall allowed"
    ));
    d.drive(Own::Hit)
}

pub(crate) fn frame_for(
    e: &mut engine::prelude::Engine,
    (id, actor, tree, snap, anim): (
        u32,
        u32,
        &[(u32, u32)],
        Option<&crate::new_physics::Snapshot>,
        &[(M3, V3)],
    ),
    slot: &mut Slot,
    np: &mut ragdoll_core::np::Np,
    (reference, take_v, floor, dt, handing): (V3, V3, f32, f32, bool),
) -> Option<Vec<(M3, V3)>> {
    use crate::new_physics::{note, note_event};
    let m = crate::new_physics::model();
    if handing {
        if slot.body.take().is_some() {
            note_event(format!(
                "{id:08X}: [np delirium] handed over (dying or given up): New Physics hands it \
                 over from Delirium's pose"
            ));
        }
        slot.queue.clear();
        return None;
    }
    // First, or again at once after a refusal, or on a new hit after it was handed back upright
    // (then from where New Physics has the body).
    if slot.body.is_none() && !slot.tried && (slot.again || !slot.back || !slot.queue.is_empty()) {
        slot.tried = true;
        // Each NPC its own settings (`delirium_bridge::profile`: neutral for now) from its facts.
        let facts = facts_of(e, actor);
        if !delirium_bridge::profile::takes(&facts) {
            note_event(format!(
                "{id:08X}: [np delirium] not taken (not a human NPC): New Physics has it as before"
            ));
            slot.queue.clear();
            return None;
        }
        let from_np = slot.again || slot.back;
        let taken = if from_np {
            take_from_np(np, reference, &facts)
        } else {
            let names: Vec<(Vec<u8>, u32)> = tree
                .iter()
                .map(|&(n, _)| (crate::active_ragdoll::node_name(e, n), n))
                .collect();
            let bone = |name: &str| -> Option<(M3, V3)> {
                let addr = names
                    .iter()
                    .find(|(n, _)| n.as_slice() == name.as_bytes())?
                    .1;
                snap.map(|s| crate::new_physics::snapshot_worlds(s, tree, &[addr])[0])
            };
            let v = [take_v[0] / UNITS, take_v[1] / UNITS, take_v[2] / UNITS];
            Dl::take(
                &bone,
                &|_| v,
                reference,
                &delirium_bridge::profile::profile(&facts),
            )
        };
        match taken {
            Ok(mut d) => {
                note_event(format!(
                    "{id:08X}: [np delirium] taken{}: Delirium has it from its pose (its own body; \
                     its facts {facts:?})",
                    if slot.again {
                        " again after a refusal, from New Physics' pose"
                    } else if slot.back {
                        " again by a new hit"
                    } else {
                        ""
                    }
                ));
                // Taken again while New Physics has it downed: down on Delirium at once.
                if from_np && np.down.is_some() {
                    d.own = Own::Down;
                    note_event(format!(
                        "{id:08X}: [np delirium] downed on Delirium (taken again)"
                    ));
                }
                // Its standing height from the animation at the first take, kept through every
                // retake (never a lying height).
                if slot.standing.is_none() {
                    let pelvis = anim.get(m.root).map_or(reference[2], |b| b.1[2]);
                    slot.standing = Some(standing_height(pelvis, floor));
                }
                slot.body = Some(d);
                slot.back = false;
                slot.again = false;
                slot.had = true;
            }
            Err(why) => {
                slot.again = false;
                note_event(format!(
                    "{id:08X}: [np delirium] not taken ({why}): New Physics has it as before{}",
                    if slot.had {
                        " (never the game's ragdoll)"
                    } else {
                        ""
                    }
                ));
                slot.queue.clear();
                return None;
            }
        }
    }
    let Some(d) = slot.body.as_mut() else {
        slot.queue.clear();
        return None;
    };
    let mut failed = None;
    let health = if slot.queue.is_empty() {
        1.0
    } else {
        health_left(e, actor)
    };
    // Each hit against the knockdown threshold (the game's topple speed for its iKnockdownThreshold,
    // the hits stacked as its impact stacking has them): past it, Delirium's own reaction, a fall
    // allowed; under it, a stagger, its muscles on New Physics' standing balance ([`Own::Stagger`]:
    // Patrick's 15:30 clip, a 9mm's 0.32 m/s went over stiff like a board; his PC run on 8a9b4a4,
    // Delirium's own stagger down about 2 s after every 9mm).
    let t = crate::gunplay::tuning();
    let topple = ragdoll_core::knock::topple_speed(
        ragdoll_core::knock::Body::Human,
        1.0,
        t.knockdown_threshold,
    );
    let half_life = t.impact_stacking.max(1) as f32 / 1000.0;
    slot.pain = stack_hit(slot.pain, 0.0, dt, half_life, topple).0;
    let (mut shots, mut knocked) = (0, false);
    for h in slot.queue.drain(..) {
        let r = match h {
            DlHit::Shot(b, point, impulse) => {
                let speed = ragdoll_core::flinch::length(impulse) / m.mass
                    * t.hit_force.max(0) as f32
                    / 100.0;
                let (pain, knock) = stack_hit(slot.pain, speed, 0.0, half_life, topple);
                slot.pain = pain;
                shots += 1;
                knocked |= knock;
                if knock {
                    if let Err(why) = toppled(id, d, slot.pain, topple) {
                        failed = Some(why);
                    }
                }
                // Under the threshold Delirium's shot task is told the actor is unhurt (its own
                // stagger, never its fall); past it, the health the actor has left.
                d.shot(b, point, impulse, if knock { health } else { 1.0 })
            }
            DlHit::Blast(c, r, speed) => {
                knocked = true;
                shots += 1;
                if let Err(why) = toppled(id, d, slot.pain, topple) {
                    failed = Some(why);
                }
                d.blast(c, r, speed)
            }
        };
        if let Err(why) = r {
            failed = Some(why);
        }
    }
    if shots > 0 && d.own != Own::Hit && !d.killed {
        note_event(format!(
            "{id:08X}: [np delirium] {shots} hit(s) while {} ({:.2} m/s stacked): Delirium's body \
             takes them, New Physics' {} goes on",
            match d.own {
                Own::Down => "down",
                Own::Rise => "rising",
                Own::Stagger { .. } => "staggering",
                _ => "held standing",
            },
            slot.pain,
            match d.own {
                Own::Down | Own::Rise => "settle and rise",
                _ => "standing balance",
            }
        ));
    }
    if shots > 0 && d.own == Own::Hit && !d.killed {
        if knocked {
            note_event(format!(
                "{id:08X}: [np delirium] a hit past the knockdown threshold ({:.2} of {topple:.2} \
                 m/s stacked, {:.0} of {:.0} health to Delirium): Delirium's own reaction, a fall \
                 allowed",
                slot.pain,
                delirium_bridge::agent::ped_health(health),
                delirium_bridge::agent::FULL_HEALTH
            ));
        } else if np.down.is_none() {
            d.own = Own::Stagger {
                since: 0.0,
                still: 0.0,
            };
            if let Err(why) = d.drive(d.own) {
                failed.get_or_insert(why);
            }
            note_event(format!(
                "{id:08X}: [np delirium] a light hit ({:.2} of {topple:.2} m/s stacked, under the \
                 knockdown threshold): staggering on Delirium (its muscles on New Physics' standing \
                 balance, its catching steps)",
                slot.pain
            ));
        }
    }
    // New Physics' bodies are simulated about its origin (`Np::origin`): their places in the world.
    let centres: Vec<V3> = m
        .node_worlds(&np.states)
        .iter()
        .map(|w| ragdoll_core::flinch::add(w.1, np.origin))
        .collect();
    // The ground under each body: New Physics' own floors where it has them, else the game's
    // collision cast down under each body (rocks and slopes as they are, never one flat plane at
    // the reference's height: Patrick's 21:41 clip, a body sunk into rocks).
    let cast_floors: Vec<Plane>;
    let floors: &[Plane] = if np.floors.is_empty() {
        cast_floors = centres
            .iter()
            .map(|c| {
                let h = crate::active_ragdoll::floor_under(e, *c).unwrap_or(floor);
                Plane {
                    p: [c[0] / UNITS, c[1] / UNITS, h / UNITS],
                    n: [0.0, 0.0, 1.0],
                }
            })
            .collect();
        &cast_floors
    } else {
        &np.floors
    };
    // The ground as the game's collision has it, cast as one height field around the pelvis (cast
    // again as the body moves on): Patrick's 15:30 clip, heels sinking into uneven ground and a
    // leg into a rock step, each body's floor a square of its own at its own height.
    let pelvis_now = centres.get(m.root).copied().unwrap_or(reference);
    let recast = d.ground.as_ref().map_or(true, |(c, _)| {
        ragdoll_core::flinch::length([pelvis_now[0] - c[0], pelvis_now[1] - c[1], 0.0])
            > GROUND_AGAIN
    });
    if recast {
        let g = ground_mesh(
            pelvis_now,
            d.origin,
            &|xy| floor_guess(floors, xy, floor),
            &mut |from| {
                crate::active_ragdoll::cast_world(e, from, [0.0, 0.0, -1.0], 200.0).map(|h| h[2])
            },
        );
        d.ground = Some((pelvis_now, g));
    }
    let mesh = world_mesh(
        (floors, d.ground.as_ref().map(|g| &g.1)),
        &np.body_walls,
        &np.edges,
        &centres,
        floor / UNITS,
        d.origin,
    );
    if d.frames == 0 {
        d.standing = slot
            .standing
            .unwrap_or_else(|| standing_height(pelvis_now[2], floor));
    }
    // Down, rising or standing (alive): New Physics' downed controller's frame. Down, it ran from
    // Delirium's body as it lies (its settle timed on the real body; what it drew is not drawn);
    // rising and standing, its bodies are the pose Delirium's muscles follow: its rise as it has
    // it, the arms pushing off through planted hands, its joint limits while downed.
    let alive = !d.killed;
    let follows = alive && d.own.follows();
    // Dead mid-rise, its wait: New Physics brings the limbs inside the skeleton's limits (as it
    // does for the game's ragdoll's hand-over) while Delirium's muscles still follow its pose.
    if d.dying.is_some() {
        np.to_limits = true;
    }
    let target = if !alive || d.own == Own::Hit {
        None
    } else {
        Some(np.frame(anim, floor, dt))
    };
    if let (true, Some(t)) = (follows, target.as_ref()) {
        if let Err(why) = d.follow(d.rig, t) {
            failed.get_or_insert(why);
        }
    }
    // Rising or held standing: Delirium's parts on New Physics' rise pose each step, moving as it
    // moves (the earlier frame's pose to this one's), and that pose drawn: no lag, no float, no
    // snap (Patrick's 23:16 clip: eased toward it, the body floated up seated, its hands unused).
    d.guide = match (follows, target.as_ref()) {
        (true, Some(t)) => {
            let parts = parts_from_bodies(d.rig, t, d.origin);
            let v = match d.target.as_ref() {
                Some(prev) if dt > 0.0 => parts_from_bodies(d.rig, prev, d.origin)
                    .iter()
                    .zip(&parts)
                    .map(|(a, b)| (b.translation - a.translation) / dt)
                    .collect(),
                _ => vec![Vec3::ZERO; parts.len()],
            };
            Some((parts, v))
        }
        _ => None,
    };
    d.target = if follows { target.clone() } else { None };
    // Its retakes counted from none again once it has kept a body a while.
    d.age += dt;
    if d.age > RETAKES_BACK_AFTER && (1..=RETAKES).contains(&slot.retakes) {
        slot.retakes = 0;
    }
    let step = match failed {
        Some(why) => Err(why),
        None => d.frame(&mesh, dt),
    };
    let (mut drawn, done) = match step {
        Ok(Step::Drawn(drawn)) => (drawn, None),
        Ok(Step::Done(drawn, why)) => (drawn, Some(why)),
        Err(why) => return refused(id, &why, slot, np, target),
    };
    if !drawn
        .iter()
        .all(|(r, t)| r.iter().all(|x| x.is_finite()) && t.iter().all(|x| x.is_finite()))
    {
        return refused(id, "its pose went bad", slot, np, target);
    }
    // Rising or held standing: New Physics' rise pose is the one drawn (Delirium's parts are on
    // it). Down or dead: drawn inside the skeleton's limits (never put back on Delirium's parts:
    // done each step, that fought its own solver, the writhing, the sliding and the stretched
    // limbs of Patrick's 23:16 clip).
    let was_np = d.np_for > 0.0;
    if let (true, Some(t)) = (follows, target.as_ref()) {
        drawn = t.clone();
        d.limit_in = 0.0;
        d.np_for += dt;
        d.ease = None;
    } else if d.own == Own::Down || d.corpse.is_some() {
        d.limit_in = (d.limit_in + LIMIT_RAMP * dt).min(LIMIT_PASSES as f32);
        let (inside, out, worst) = into_limits(&drawn, d.limit_in as usize);
        if out > d.limited {
            d.worst = worst;
        }
        d.limited = d.limited.max(out);
        drawn = inside;
    }
    // Back on Delirium's own body from New Physics' rise pose: the two differ a little (its parts
    // put on that pose, stepped, the skeleton made again from them), eased out over a moment,
    // never popped in a frame (the trench replay: every body 3 to 6 units at once).
    if !follows && was_np && d.last.len() == drawn.len() {
        d.ease = Some((offsets(&drawn, &d.last), 1.0));
    }
    if let Some((offs, left)) = d.ease.as_mut() {
        drawn = eased(&drawn, offs, *left);
        *left -= dt / EASE_BACK;
        if *left <= 0.0 {
            d.ease = None;
        }
    }
    if !follows {
        d.np_for = 0.0;
    }
    // Never a body flung away. A jump of a frame or two is held at the last right pose (Patrick's
    // 15:30 log: one knee's 30-unit frame refused the body mid-stagger, and the new agent made
    // from that pose went over like a board); only one that goes on refuses it.
    if let Some(why) = pose_wrong(&drawn, &d.last, d.origin, dt) {
        if d.glitch < GLITCHES && !d.last.is_empty() {
            d.glitch += 1;
            note_event(format!(
                "{id:08X}: [np delirium] a jump not drawn ({why}): the last right pose held \
                 ({} of {GLITCHES} frames)",
                d.glitch
            ));
            return Some(d.last.clone());
        }
        let why = format!("its pose went wrong: {why}");
        return refused(id, &why, slot, np, target);
    }
    d.glitch = 0;
    // Its posture and balance from Delirium's body (New Physics' own balance never runs while
    // Delirium has it: its readings would be its defaults).
    let pose = posture(&drawn, &d.last, floor, dt);
    if d.frames <= 20 {
        note(format!(
            "{id:08X}: [np delirium] frame {}: the pelvis {:.0} u over the floor (standing {:.0}), \
             the torso {:.0} degrees from upright, {:.2} m/s, the capture point {:.3} m outside \
             the feet, balancing {}",
            d.frames,
            pose.pelvis_up,
            d.standing,
            pose.lean,
            pose.speed,
            pose.outside,
            d.agent_balancing()
        ));
    }
    // New Physics' bodies on Delirium's pose while Delirium's own reactions have it (hit, lying,
    // dead: what reads them sees the real body), moving as it moves; while its muscles follow New
    // Physics' rise, New Physics' bodies are the pose followed and stay its own.
    let moving = velocities(&drawn, &d.last, dt);
    if !follows {
        put_on(np, &drawn, &moving);
    }
    d.last = drawn.clone();
    if d.limited > LIMIT_HOLD && d.frames % 30 == 0 {
        note(format!(
            "{id:08X}: [np delirium] joints drawn inside the skeleton's limits (the most out {:.1} \
             degrees, {})",
            d.limited.to_degrees(),
            d.worst
        ));
        d.limited = 0.0;
    }
    let said = |what: &str, pose: &Posture, d: &Dl| {
        format!(
            "{id:08X}: [np delirium] {what} after {} frames (the pelvis {:.0} u over the floor of \
             {:.0} standing, the torso {:.0} degrees from upright, {:.2} m/s)",
            d.frames, pose.pelvis_up, d.standing, pose.lean, pose.speed
        )
    };
    if !alive {
        return Some(drawn);
    }
    if let Some(n) = d.dying {
        let out = np.report.limit_most;
        let inside = np.on_data_limits && out <= crate::new_physics::HAND_LIMITS;
        if wait_over(n + 1, np.on_data_limits, out) {
            note_event(format!(
                "{id:08X}: [np delirium] dead on Delirium mid-rise: its wait over after {} frames \
                 (the limbs {}, at most {:.1} degrees out): its muscles off",
                n + 1,
                if inside {
                    "brought inside the skeleton's limits"
                } else {
                    "not all inside the skeleton's limits at the longest wait"
                },
                out.to_degrees()
            ));
            if let Err(why) = d.kill() {
                return refused(id, &why, slot, np, Some(drawn));
            }
        } else {
            d.dying = Some(n + 1);
        }
        return Some(drawn);
    }
    if d.own == Own::Hit {
        // Down (fallen over, on the ground, or lying settled): Delirium keeps it; New Physics'
        // downed controller is made at once (never "falling, not yet down", which the host hands
        // to the game's ragdoll: Patrick's 23:11 log) and reads the body as it lies.
        if done == Some("it lies settled") || pose.downed(d.standing) {
            note_event(format!(
                "{}: downed on Delirium (Delirium keeps the body; New Physics' downed controller \
                 reads it as it lies, its settle, then its rise on Delirium's muscles)",
                said("downed", &pose, d),
            ));
            np.fall_now("downed on Delirium");
            d.own = Own::Down;
        } else if done.is_some() {
            // It stood balanced again two seconds: upright, still, its animation's again.
            note_event(format!(
                "{}: upright and back to the game's animation (it stood balanced again)",
                said("upright", &pose, d)
            ));
            slot.body = None;
            slot.tried = false;
            slot.back = true;
        }
        return Some(drawn);
    }
    // Rising, upright and still when New Physics gives its rise up (its stand never read, its
    // feet's load coming and going): it stood all the same. Taken for a failed rise, the body was
    // dropped from standing ("down again" with the pelvis at 68 of 67 units and the torso 6
    // degrees from upright: Patrick's PC run on 5f52c0e, eight times).
    if d.own == Own::Rise
        && !np.rise
        && np.down.is_some()
        && pose.upright(d.standing)
        && pose.speed < UP_STILL
    {
        np.stand_now(anim);
        note_event(format!(
            "{id:08X}: [np delirium] upright and still as New Physics gave its rise up: stood \
             (never dropped from standing)"
        ));
    }
    let (next, change) = next_own(
        d.own,
        np.down.is_some(),
        np.rise,
        pose.upright(d.standing),
        pose.speed,
        dt,
    );
    let was = d.own;
    d.own = next;
    let Some(change) = change else {
        return Some(drawn);
    };
    let driven = match change {
        Change::Rising => {
            note_event(format!(
                "{}: rising on Delirium (its muscles on New Physics' rise, the arms pushing off \
                 through planted hands)",
                said("rising", &pose, d)
            ));
            if was == Own::Down {
                d.drive(Own::Rise)
            } else {
                Ok(())
            }
        }
        Change::BackDown => {
            note_event(format!(
                "{}: back down on Delirium (New Physics' rise stopped; lying relaxed until it \
                 rises again)",
                said("down again", &pose, d)
            ));
            // New Physics' bodies on the body as it lies again.
            put_on(np, &drawn, &moving);
            d.drive(Own::Down)
        }
        // New Physics' rise is done. Delirium's body at least half up (the pelvis at 60% of its
        // height, the torso within 45 degrees): held on New Physics' standing pose until it is
        // upright and still. Still down: down again on Delirium, to settle and rise again (never
        // called standing while it lies).
        Change::Standing if pose.pelvis_up < 0.6 * d.standing || pose.lean > 45.0 => {
            note_event(format!(
                "{}: New Physics' rise ended with Delirium's body still down: down again on \
                 Delirium, to settle and rise again",
                said("still down", &pose, d)
            ));
            d.own = Own::Down;
            put_on(np, &drawn, &moving);
            np.fall_now("Delirium's body still down");
            d.drive(Own::Down)
        }
        Change::Standing => {
            note_event(format!(
                "{}: New Physics stood and Delirium's body is coming up: held on New Physics' \
                 standing pose until it is upright and still",
                said("coming up", &pose, d)
            ));
            if was == Own::Down {
                d.drive(next)
            } else {
                Ok(())
            }
        }
        // Staggering, New Physics' balance went down: down on Delirium, relaxed from the pose it
        // had (its own fall from there), New Physics' downed controller reading it as it lies.
        Change::Fell => {
            note_event(format!(
                "{}: New Physics' standing balance went down: down on Delirium (its own fall from \
                 there, then New Physics' settle and rise)",
                said("downed staggering", &pose, d)
            ));
            put_on(np, &drawn, &moving);
            d.drive(Own::Down)
        }
        Change::Upright => {
            note_event(format!(
                "{}: upright and still: back to the game's animation (New Physics' standing \
                 balance from Delirium's pose lets it go)",
                said("upright", &pose, d)
            ));
            put_on(np, &drawn, &moving);
            slot.body = None;
            slot.tried = false;
            slot.back = true;
            return Some(drawn);
        }
        // Not upright UP_MOST seconds after New Physics stood: down again on Delirium (its own
        // body kept), to settle and rise again.
        Change::Stuck => {
            note_event(format!(
                "{}: not upright {:.0} s after New Physics {}: down again on Delirium, to settle \
                 and rise again",
                said("not upright", &pose, d),
                if matches!(was, Own::Stagger { .. }) {
                    STAGGER_MOST
                } else {
                    UP_MOST
                },
                if matches!(was, Own::Stagger { .. }) {
                    "took the stagger"
                } else {
                    "stood"
                }
            ));
            d.own = Own::Down;
            put_on(np, &drawn, &moving);
            np.fall_now("Delirium's body didn't stand");
            d.drive(Own::Down)
        }
    };
    if let Err(why) = driven {
        return refused(id, &why, slot, np, target);
    }
    Some(drawn)
}

/// Whether a death mid-rise's wait is over after `frames` of it: the limbs inside the skeleton's
/// limits (New Physics on them, `out` radians out at most), or the longest wait, as New Physics'
/// own death hand-over has them.
pub(crate) fn wait_over(frames: u32, on_limits: bool, out: f32) -> bool {
    (on_limits && out <= crate::new_physics::HAND_LIMITS)
        || frames >= crate::new_physics::HAND_WAIT_FRAMES
}

/// Delirium refused or failed the actor (`why`, said in the log): New Physics carries it this
/// frame as it is (hit: its own frame from the pose it had, None; downed, rising or standing: its
/// downed controller's frame `target`), and Delirium takes it again at once from New Physics'
/// pose, [`RETAKES`] times at most; after that New Physics keeps it (never the game's ragdoll:
/// the host keeps every body Delirium had from it).
fn refused(
    id: u32,
    why: &str,
    slot: &mut Slot,
    np: &ragdoll_core::np::Np,
    target: Option<Vec<(M3, V3)>>,
) -> Option<Vec<(M3, V3)>> {
    let own = slot.body.take().map(|d| (d.own, d.killed));
    let had = match own {
        Some((_, true)) => "dead",
        Some((Own::Hit, _)) | None => "hit",
        Some((Own::Down, _)) => "downed",
        Some((Own::Rise, _)) => "rising",
        Some((Own::Up { .. }, _)) => "standing",
        Some((Own::Stagger { .. }, _)) => "staggering",
    };
    let then = if slot.retakes < RETAKES {
        slot.tried = false;
        slot.again = true;
        format!(
            "New Physics carries it this frame, then Delirium takes it again from New Physics' \
             pose (retake {} of {RETAKES})",
            slot.retakes + 1
        )
    } else {
        format!(
            "{RETAKES} retakes used: New Physics keeps it as it is (its downed controller, its \
             rise; never the game's ragdoll)"
        )
    };
    slot.retakes = slot.retakes.saturating_add(1);
    crate::new_physics::note_event(format!(
        "{id:08X}: [np delirium] refused while {had} ({why}): {then}"
    ));
    slot.down = np.down.is_some();
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Delirium's parts in FNV's bind pose give New Physics' bodies FNV's bind pose: every body's
    /// node within 0.01 units and 0.05 degrees (the conversions both ways and the rig's pose up
    /// the skeleton), wherever the origin is.
    #[test]
    fn the_bind_pose_comes_back_as_new_physics_bodies() {
        let s = skel();
        let names: Vec<&str> = s.names.iter().map(String::as_str).collect();
        let bones = delirium_bridge::rig::part_bones(&names);
        let parts: Vec<Transform> = (0..21)
            .map(|p| s.rest[bones[p].or(bones[7]).unwrap()])
            .collect();
        let rig = Rig::new(&names, &s.parents, &s.rest, &parts).unwrap();
        let origin = [-72152.0, 8282.0, 223.0];
        // The parts placed at the origin (as an actor's are, measured from its reference).
        let drawn = bodies_from_parts(&rig, &parts, origin);
        let sk = ragdoll_core::np_skeleton::Skeleton::fnv();
        let bind = sk.bind_worlds();
        let m = crate::new_physics::model();
        assert_eq!(drawn.len(), m.bodies.len());
        for (b, (r, t)) in m.bodies.iter().zip(&drawn) {
            let i = sk.node(&b.node).unwrap();
            let want_t = [
                bind[i].t[0] + origin[0],
                bind[i].t[1] + origin[1],
                bind[i].t[2] + origin[2],
            ];
            let d = ragdoll_core::flinch::length(ragdoll_core::flinch::sub(*t, want_t));
            assert!(d < 0.01, "{}: {d} units off", b.node);
            let a = ragdoll_core::np_skeleton::angle_between(r, &bind[i].r).to_degrees();
            assert!(a < 0.05, "{}: {a} degrees off", b.node);
        }
    }

    /// A node's world through Delirium's transform and back: the same, any turn, far from the
    /// world's centre.
    #[test]
    fn a_world_goes_there_and_back() {
        let origin = [-72152.0, 8282.0, 223.0];
        let q = Quat::from_euler(
            delirium_bridge::delirium::prelude::EulerRot::XYZ,
            0.4,
            -1.1,
            2.0,
        );
        let r = ragdoll_core::active::m_from_q([q.w, q.x, q.y, q.z]);
        let t = [-72100.5, 8300.25, 260.0];
        let tf = to_tf(&r, t, origin);
        let (r2, t2) = from_tf(&tf, origin);
        assert!(ragdoll_core::flinch::length(ragdoll_core::flinch::sub(t, t2)) < 0.01);
        assert!(ragdoll_core::np_skeleton::angle_between(&r, &r2).to_degrees() < 0.01);
    }

    /// The actor's facts: an NPC's base form with its weight slider, or a creature's; its health
    /// left from its owner's current and base values (1 when it has no base).
    #[test]
    fn an_actors_facts_and_health_left() {
        use delirium_bridge::profile::Actor;
        use engine::prelude::*;
        const AREA: u32 = 0x7600_0000;
        const ACTOR: u32 = AREA;
        const BASE: u32 = AREA + 0x1000;
        const OWNER_VT: u32 = AREA + 0x2000;
        const CURRENT_FN: u32 = AREA + 0x3000;
        const BASE_FN: u32 = AREA + 0x3004;
        let mut e = Engine::new();
        e.mem.map(AREA, 0x4000);
        e.mem.set_u32(ACTOR + BASE_FORM, BASE);
        e.mem.set_u8(BASE + FORM_TYPE, NPC_FORM);
        // The slider as the form keeps it, 0 to 1 (Patrick's Powder Ganger read 0 as 0 to 100).
        e.mem.set_f32(BASE + NPC_WEIGHT, 0.8);
        let f = facts_of(&mut e, ACTOR);
        assert_eq!((f.actor, f.weight), (Actor::Npc, 80));
        e.mem.set_u8(BASE + FORM_TYPE, 0x2b);
        assert_eq!(facts_of(&mut e, ACTOR).actor, Actor::Creature);
        assert!(!delirium_bridge::profile::takes(&facts_of(&mut e, ACTOR)));
        // Its actor values: current 30 of a base 120.
        e.mem.set_u32(ACTOR + AV_OWNER, OWNER_VT);
        e.mem
            .set_u32(OWNER_VT + engine::actor_values::CURRENT, CURRENT_FN);
        e.mem.set_u32(OWNER_VT + AV_BASE, BASE_FN);
        e.register(CURRENT_FN, |_, _| 30.0f32.into_ret());
        e.register(BASE_FN, |_, _| 120.0f32.into_ret());
        assert!((health_left(&mut e, ACTOR) - 0.25).abs() < 1e-6);
        e.register(BASE_FN, |_, _| 0.0f32.into_ret());
        assert_eq!(health_left(&mut e, ACTOR), 1.0);
    }

    /// The game's death on a ragdoll: a corpse Delirium keeps keeps its ragdoll following the pose
    /// drawn (`DisableRagdollAnim` leaves it as it was); every other ragdoll is turned loose as
    /// the game has it; a corpse dropped is the game's again.
    #[test]
    fn the_games_death_turns_only_other_ragdolls_loose() {
        use engine::prelude::*;
        const AREA: u32 = 0x7700_0000;
        const ACTOR: u32 = AREA;
        const KEPT: u32 = AREA + 0x1000;
        const OTHER: u32 = AREA + 0x2000;
        let mut e = Engine::new();
        e.mem.map(AREA, 0x4000);
        // The exe's vector the game's version copies (`010c71b0`): zero in the fixture.
        e.mem.map(0x010c_7000, 0x1000);
        install(&mut e);
        for c in [KEPT, OTHER] {
            // Its ragdoll animation on (`+0x42`), the bytes the game's clears set.
            e.mem.set_u8(c + 0x42, 1);
            e.mem.set_u8(c + 0x41, 1);
            e.mem.set_u8(c + 0x43, 1);
        }
        e.mem.set_u32(ACTOR + RAGDOLL_CONTROLLER, KEPT);
        let hold = CorpseHold::of(&e, ACTOR).unwrap();
        e.call(DISABLE_RAGDOLL_ANIM, &args![KEPT, 1u32]);
        e.call(DISABLE_RAGDOLL_ANIM, &args![OTHER, 0u32]);
        assert_eq!(
            (e.mem.u8(KEPT + 0x41), e.mem.u8(KEPT + 0x43)),
            (1, 1),
            "the kept one"
        );
        assert_eq!(
            (e.mem.u8(OTHER + 0x41), e.mem.u8(OTHER + 0x43)),
            (0, 0),
            "the other"
        );
        drop(hold);
        e.call(DISABLE_RAGDOLL_ANIM, &args![KEPT, 0u32]);
        assert_eq!(e.mem.u8(KEPT + 0x41), 0, "dropped: the game's again");
    }

    /// New Physics' bodies in FNV's bind pose (game units), turned by `r` about the pelvis and
    /// moved by `by`.
    fn bodies_posed(r: M3, by: V3) -> Vec<(M3, V3)> {
        let sk = ragdoll_core::np_skeleton::Skeleton::fnv();
        let bind = sk.bind_worlds();
        let m = crate::new_physics::model();
        let pelvis = bind[sk.node(&m.bodies[m.root].node).unwrap()].t;
        m.bodies
            .iter()
            .map(|b| {
                let w = &bind[sk.node(&b.node).unwrap()];
                let off = ragdoll_core::flinch::sub(w.t, pelvis);
                (
                    ragdoll_core::flinch::mul(&r, &w.r),
                    ragdoll_core::flinch::add(
                        ragdoll_core::flinch::add(pelvis, ragdoll_core::flinch::apply_m(&r, off)),
                        by,
                    ),
                )
            })
            .collect()
    }

    const ID: M3 = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

    /// Standing in FNV's bind pose: the pelvis at its standing height, upright, still, the capture
    /// point over the feet: not down. The same body lying on its back (turned 90 degrees about its
    /// side, on the floor) is down; so is one crouched with its pelvis at a third of its height.
    #[test]
    fn delirium_s_pose_reads_standing_or_down() {
        let stand = bodies_posed(ID, [0.0; 3]);
        let m = crate::new_physics::model();
        let feet = ["Bip01 L Foot", "Bip01 R Foot"].map(|n| stand[m.body(n).unwrap()].1[2]);
        let floor = feet[0].min(feet[1]) - 5.0;
        let p = posture(&stand, &stand, floor, 1.0 / 60.0);
        let standing = p.pelvis_up;
        assert!(standing > 40.0, "{p:?}");
        assert!(p.lean < 15.0 && p.speed < 1e-4 && p.outside < 0.05, "{p:?}");
        assert!(!p.downed(standing));
        // Lying: turned about x by 90 degrees, its pelvis 8 units over the floor.
        let lie: M3 = [1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0];
        let pelvis = stand[m.root].1;
        let lying = bodies_posed(lie, [0.0, 0.0, floor + 8.0 - pelvis[2]]);
        let p = posture(&lying, &lying, floor, 1.0 / 60.0);
        assert!(p.lean > 80.0 && p.pelvis_up < 10.0, "{p:?}");
        assert!(p.downed(standing));
        // Sunk to a third of its height, upright: down too.
        let low = bodies_posed(ID, [0.0, 0.0, -standing * 0.67]);
        assert!(posture(&low, &low, floor, 1.0 / 60.0).downed(standing));
    }

    /// The velocities handed to New Physics: a pose moved 7 units in a 60th of a second goes at
    /// 6 m/s; one turned 0.1 rad about z spins at 6 rad/s about z; and the speed in the posture
    /// is the centre of mass's.
    #[test]
    fn the_hand_over_moves_as_delirium_moved() {
        let dt = 1.0 / 60.0;
        let a = bodies_posed(ID, [0.0; 3]);
        let b = bodies_posed(ID, [7.0, 0.0, 0.0]);
        for (v, w) in velocities(&b, &a, dt) {
            assert!(
                (v[0] - 6.0).abs() < 1e-3 && v[1].abs() < 1e-3 && v[2].abs() < 1e-3,
                "{v:?}"
            );
            assert!(ragdoll_core::flinch::length(w) < 1e-3, "{w:?}");
        }
        let (c, s) = (0.1f32.cos(), 0.1f32.sin());
        let turned = bodies_posed([c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0], [0.0; 3]);
        let w = velocities(&turned, &a, dt)[0].1;
        assert!(
            (w[2] - 6.0).abs() < 0.05 && w[0].abs() < 0.05 && w[1].abs() < 0.05,
            "{w:?}"
        );
        let p = posture(&b, &a, 0.0, dt);
        assert!((p.speed - 6.0).abs() < 1e-3, "{p:?}");
    }

    /// New Physics' bodies put on a pose of Delirium's far from the world's centre, as Patrick's
    /// actors are (-72152, 8282): New Physics draws that pose back where it is, to the hundredth of
    /// a unit, moving as given; its next frame of its own carries on from there, never thousands
    /// of units off.
    #[test]
    fn new_physics_takes_the_pose_where_it_is() {
        let at = [-72152.0, 8282.0, 6440.0];
        let anim = bodies_posed(ID, at);
        let mut np = ragdoll_core::np::Np::new(crate::new_physics::model().clone(), &anim, 1.0);
        assert!(
            ragdoll_core::flinch::length(np.origin) > 1000.0,
            "{:?}",
            np.origin
        );
        // Delirium's pose: the body moved 3 units and turned a little.
        let (c, s) = (0.05f32.cos(), 0.05f32.sin());
        let drawn = bodies_posed(
            [c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0],
            [at[0] + 3.0, at[1], at[2]],
        );
        let moving = vec![([0.5, 0.0, 0.0], [0.0, 0.0, 1.0]); drawn.len()];
        put_on(&mut np, &drawn, &moving);
        let m = crate::new_physics::model();
        let back = m.node_worlds(&np.states);
        for (b, ((_, t), (_, want))) in back.iter().zip(&drawn).enumerate() {
            let t = ragdoll_core::flinch::add(*t, np.origin);
            let d = ragdoll_core::flinch::length(ragdoll_core::flinch::sub(t, *want));
            assert!(d < 0.01, "{}: {d} units off", m.bodies[b].node);
        }
        assert!(np
            .states
            .iter()
            .all(|s| s.v == [0.5, 0.0, 0.0] && s.w == [0.0, 0.0, 1.0]));
        // New Physics' own frame from there: the body where Delirium left it (moved by its own
        // physics a little in a frame, never by thousands of units).
        let next = np.frame(&drawn, at[2] - 80.0, 1.0 / 60.0);
        let pelvis = next[m.root].1;
        let off = ragdoll_core::flinch::length(ragdoll_core::flinch::sub(pelvis, drawn[m.root].1));
        assert!(
            off < 5.0,
            "the pelvis {off} units off after New Physics' frame"
        );
    }

    /// Handed down from Delirium far from the world's centre: New Physics' own controller has the
    /// body downed at once (never "falling, not yet down", which the host hands to the game's
    /// ragdoll), and keeps it downed, where it was put, through its next frames.
    #[test]
    fn a_body_handed_down_is_new_physics_downed() {
        let at = [-72152.0, 8282.0, 6440.0];
        let anim = bodies_posed(ID, at);
        let mut np = ragdoll_core::np::Np::new(crate::new_physics::model().clone(), &anim, 1.0);
        np.keep_down = true;
        let lie: M3 = [1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0];
        let m = crate::new_physics::model();
        let floor = at[2] - 60.0;
        // Lying at the actor's place, the pelvis 10 units over the floor (the bind pose's pelvis
        // is `anim`'s less `at`).
        let bind_pelvis_z = anim[m.root].1[2] - at[2];
        let drawn = bodies_posed(lie, [at[0], at[1], floor + 10.0 - bind_pelvis_z]);
        put_on(&mut np, &drawn, &vec![([0.0; 3], [0.0; 3]); drawn.len()]);
        np.fall_now("downed on Delirium");
        assert!(np.down.is_some());
        assert_eq!(np.phase(), ragdoll_core::np_balance::Phase::Falling);
        let mut last = drawn[m.root].1;
        for _ in 0..30 {
            let out = np.frame(&anim, floor, 1.0 / 60.0);
            assert!(np.down.is_some(), "no longer downed");
            let p = out[m.root].1;
            let moved = ragdoll_core::flinch::length(ragdoll_core::flinch::sub(p, last));
            assert!(moved < 10.0, "the pelvis moved {moved} units in a frame");
            assert!(
                p[2] > floor - 5.0,
                "the pelvis {} under the floor",
                floor - p[2]
            );
            last = p;
        }
    }

    /// The pose Delirium's muscles follow on the get-up: New Physics' bodies made into
    /// Delirium's parts give the parts back (the bones that are not New Physics' bodies, the
    /// pelvis, the clavicles and the neck, under their parents as in the bind pose), within a
    /// hundredth of a unit and a tenth of a degree, the body anywhere, turned anyhow.
    #[test]
    fn new_physics_bodies_give_delirium_the_pose_to_follow() {
        let s = skel();
        let names: Vec<&str> = s.names.iter().map(String::as_str).collect();
        let bones = delirium_bridge::rig::part_bones(&names);
        let bind: Vec<Transform> = (0..21)
            .map(|p| s.rest[bones[p].or(bones[7]).unwrap()])
            .collect();
        let rig = Rig::new(&names, &s.parents, &s.rest, &bind).unwrap();
        let origin = [-72152.0, 8282.0, 223.0];
        let turn = Quat::from_euler(
            delirium_bridge::delirium::prelude::EulerRot::XYZ,
            1.2,
            -0.4,
            2.5,
        );
        let moved = Transform::from_translation(Vec3::new(1.5, -0.7, 0.3)).with_rotation(turn);
        for parts in [
            bind.clone(),
            bind.iter().map(|p| moved * *p).collect::<Vec<_>>(),
        ] {
            let bodies = bodies_from_parts(&rig, &parts, origin);
            let back = parts_from_bodies(&rig, &bodies, origin);
            assert_eq!(back.len(), 21);
            for (k, (a, b)) in parts.iter().zip(&back).enumerate() {
                let d = (a.translation - b.translation).length() * UNITS;
                assert!(d < 0.01, "part {k}: {d} units off");
                let deg = a.rotation.angle_between(b.rotation).to_degrees();
                assert!(deg < 0.1, "part {k}: {deg} degrees off");
            }
        }
    }

    /// The get-up on Delirium (Patrick's plan, never the game's ragdoll): down while New Physics'
    /// controller settles; rising once it rises; down again when the rise stops; once New
    /// Physics stands, held until Delirium's body is upright and still for UP_STILL_FOR, then back
    /// to the animation; not upright UP_MOST seconds after, New Physics has it; falling again
    /// while held, down or rising again. A hit body stays Delirium's own.
    #[test]
    fn the_get_up_on_delirium_settles_rises_and_ends_upright_and_still() {
        let dt = 1.0 / 60.0;
        assert_eq!(
            next_own(Own::Hit, true, true, false, 1.0, dt),
            (Own::Hit, None)
        );
        assert_eq!(
            next_own(Own::Down, true, false, false, 0.0, dt),
            (Own::Down, None)
        );
        assert_eq!(
            next_own(Own::Down, true, true, false, 0.0, dt),
            (Own::Rise, Some(Change::Rising))
        );
        assert_eq!(
            next_own(Own::Rise, true, true, false, 0.5, dt),
            (Own::Rise, None)
        );
        assert_eq!(
            next_own(Own::Rise, true, false, false, 0.5, dt),
            (Own::Down, Some(Change::BackDown))
        );
        let (mut own, c) = next_own(Own::Rise, false, false, true, 0.5, dt);
        assert_eq!(c, Some(Change::Standing));
        // Upright but still moving: held; still (and upright) long enough: back to the animation.
        let mut t = 0.0;
        for _ in 0..30 {
            let (n, c) = next_own(own, false, false, true, 0.4, dt);
            assert_eq!(c, None);
            own = n;
            t += dt;
        }
        let mut upright_at = None;
        for i in 0..60 {
            let (n, c) = next_own(own, false, false, true, 0.05, dt);
            own = n;
            t += dt;
            if c == Some(Change::Upright) {
                upright_at = Some(i);
                break;
            }
            assert_eq!(c, None);
        }
        let i = upright_at.expect("never back to the animation");
        assert!((i as f32 + 1.0) * dt >= UP_STILL_FOR - 1e-4 && t < UP_MOST);
        // Never upright: New Physics has it UP_MOST seconds after it stood.
        let mut own = Own::Up {
            since: 0.0,
            still: 0.0,
        };
        let mut stuck = None;
        for i in 0..400 {
            let (n, c) = next_own(own, false, false, false, 0.0, dt);
            own = n;
            if c.is_some() {
                stuck = Some((i, c));
                break;
            }
        }
        let (i, c) = stuck.unwrap();
        assert_eq!(c, Some(Change::Stuck));
        assert!(((i + 1) as f32 * dt - UP_MOST).abs() < 2.0 * dt);
        // Fallen again while held.
        let up = Own::Up {
            since: 1.0,
            still: 0.1,
        };
        assert_eq!(
            next_own(up, true, false, false, 1.0, dt),
            (Own::Down, Some(Change::BackDown))
        );
        assert_eq!(
            next_own(up, true, true, false, 1.0, dt),
            (Own::Rise, Some(Change::Rising))
        );
    }

    /// A light hit's stagger on New Physics' standing balance: held at least STAGGER_LEAST
    /// however still it is, then back to the animation upright and still; down on Delirium the
    /// moment New Physics' balance goes down; upright after STAGGER_MOST all the same, else down.
    #[test]
    fn a_stagger_is_held_on_new_physics_balance() {
        let dt = 1.0 / 100.0;
        let mut own = Own::Stagger {
            since: 0.0,
            still: 0.0,
        };
        assert!(own.follows());
        let mut back = None;
        for i in 0..1000 {
            let (n, c) = next_own(own, false, false, true, 0.0, dt);
            own = n;
            if c.is_some() {
                back = Some((i, c));
                break;
            }
        }
        let (i, c) = back.unwrap();
        assert_eq!(c, Some(Change::Upright));
        assert!(
            ((i + 1) as f32 * dt - STAGGER_LEAST).abs() < 2.0 * dt,
            "{i}"
        );
        let staggering = Own::Stagger {
            since: 0.5,
            still: 0.0,
        };
        assert_eq!(
            next_own(staggering, true, false, false, 1.0, dt),
            (Own::Down, Some(Change::Fell))
        );
        let long = Own::Stagger {
            since: STAGGER_MOST,
            still: 0.0,
        };
        assert_eq!(
            next_own(long, false, false, true, 1.0, dt).1,
            Some(Change::Upright)
        );
        assert_eq!(
            next_own(long, false, false, false, 1.0, dt).1,
            Some(Change::Stuck)
        );
    }

    /// Upright is stricter than not downed: a body crouched at 70% of its height, or leaning 30
    /// degrees, is neither down nor upright.
    #[test]
    fn upright_is_tall_and_straight() {
        let p = |up: f32, lean: f32| Posture {
            pelvis_up: up,
            lean,
            speed: 0.0,
            outside: 0.0,
            com: [0.0; 3],
            v: [0.0; 3],
            feet: [0.0; 3],
        };
        assert!(p(64.0, 5.0).upright(67.0));
        assert!(!p(47.0, 5.0).upright(67.0) && !p(47.0, 5.0).downed(67.0));
        assert!(!p(64.0, 30.0).upright(67.0) && !p(64.0, 30.0).downed(67.0));
        assert!(p(20.0, 80.0).downed(67.0) && !p(20.0, 80.0).upright(67.0));
    }

    /// A refused body is taken again from New Physics' pose at once, RETAKES times; then New
    /// Physics keeps it (and the host still keeps it from the game's ragdoll: `had` stays). The
    /// frame New Physics carries it is the one given back.
    #[test]
    fn a_refused_body_is_taken_again_then_kept_by_new_physics() {
        let anim = bodies_posed(ID, [100.0, 200.0, 300.0]);
        let mut np = ragdoll_core::np::Np::new(crate::new_physics::model().clone(), &anim, 1.0);
        np.fall_now("test");
        let mut slot = Slot {
            tried: true,
            had: true,
            ..Default::default()
        };
        for k in 1..=RETAKES {
            let back = refused(0x1234, "test", &mut slot, &np, Some(anim.clone()));
            assert_eq!(back.as_deref(), Some(anim.as_slice()));
            assert!(slot.again && !slot.tried, "retake {k}");
            assert_eq!(slot.retakes, k);
            assert!(slot.down && slot.had);
            // As if the retake was made.
            slot.again = false;
            slot.tried = true;
        }
        assert!(refused(0x1234, "test", &mut slot, &np, None).is_none());
        assert!(!slot.again && slot.tried && slot.had, "kept by New Physics");
        assert!(slot.retakes > RETAKES);
    }

    /// A death mid-rise waits until the limbs are inside the skeleton's limits, at most New
    /// Physics' own death hand-over's longest wait.
    #[test]
    fn a_death_mid_rise_waits_for_the_limbs() {
        use crate::new_physics::{HAND_LIMITS, HAND_WAIT_FRAMES};
        assert!(!wait_over(1, false, 0.0));
        assert!(!wait_over(1, true, HAND_LIMITS * 2.0));
        assert!(wait_over(1, true, HAND_LIMITS * 0.5));
        assert!(!wait_over(HAND_WAIT_FRAMES - 1, true, 1.0));
        assert!(wait_over(HAND_WAIT_FRAMES, false, 1.0));
    }

    /// A downed body's pose with a knee bent the wrong way (Patrick's 15:30 clip: a leg through
    /// the pelvis) is brought inside the skeleton's limits; the bind pose is left as it is.
    #[test]
    fn a_pose_out_of_the_limits_is_brought_in() {
        let m = crate::new_physics::model();
        let at = [-72152.0, 8282.0, 223.0];
        let bind = bodies_posed(ID, at);
        let (same, out, _) = into_limits(&bind, 96);
        assert!(out <= LIMIT_HOLD, "the bind pose {out} rad out");
        assert_eq!(same, bind);
        // The left calf turned 120 degrees about its own place.
        let calf = m.body("Bip01 L Calf").unwrap();
        let (c, s2) = (120f32.to_radians().cos(), 120f32.to_radians().sin());
        let turn: M3 = [1.0, 0.0, 0.0, 0.0, c, -s2, 0.0, s2, c];
        let mut bent = bind.clone();
        bent[calf].0 = ragdoll_core::flinch::mul(&turn, &bent[calf].0);
        let (inside, out, worst) = into_limits(&bent, 96);
        assert!(worst.contains("Calf") || worst.contains("Foot"), "{worst}");
        assert!(out > 0.5, "the bent knee only {out} rad out");
        let local: Vec<(M3, V3)> = inside
            .iter()
            .map(|(r, t)| (*r, ragdoll_core::flinch::sub(*t, inside[m.root].1)))
            .collect();
        let after = m
            .data_limits_out(&m.states_from_nodes(&local))
            .into_iter()
            .fold(0.0f32, f32::max);
        assert!(after < 0.1, "still {after} rad out after");
    }

    /// The ground cast as one height field: each point cast from 100 units over the body first (a
    /// trench's bank 60 units over the floor guessed is met, which a cast from 40 over the guess
    /// started inside: Patrick's 23:16 clip), then from 40 over the guess where that met nothing,
    /// the guess where neither met anything; a rock step's top where the casts meet it, measured
    /// from the origin.
    #[test]
    fn the_ground_is_cast_as_one_height_field() {
        let origin = [-72152.0, 8282.0, 200.0];
        let centre = [-72152.0, 8282.0, 260.0];
        let guess_at = |y: f32| 200.0 + 0.1 * (y - centre[1]);
        // The ground: nothing past 60 units on +x, a rock step's top to 60, a bank 60 over the
        // guess under -60, the guessed slope between; a cast meets it only from above.
        let ground = |x: f32, y: f32| -> Option<f32> {
            if x > centre[0] + 60.0 {
                None
            } else if x > centre[0] {
                Some(230.0)
            } else if x < centre[0] - 60.0 {
                Some(guess_at(y) + 60.0)
            } else {
                Some(guess_at(y))
            }
        };
        let mut froms = Vec::new();
        let mut cast = |from: V3| {
            froms.push(from);
            ground(from[0], from[1]).filter(|&z| z <= from[2])
        };
        let g = ground_mesh(centre, origin, &|xy| guess_at(xy[1]), &mut cast);
        assert_eq!(g.positions.len(), GROUND_N * GROUND_N);
        assert_eq!(g.triangles.len(), 2 * (GROUND_N - 1) * (GROUND_N - 1));
        let mut misses = 0;
        let mut i = 0;
        for p in &g.positions {
            let from = froms[i];
            let guess = guess_at(from[1]);
            assert!(
                (from[2] - (centre[2].max(guess) + 100.0)).abs() < 1e-3,
                "{from:?}"
            );
            i += 1;
            let want = match ground(from[0], from[1]) {
                Some(z) => z,
                None => {
                    // Met nothing: cast again from over the guess, then the guess.
                    let again = froms[i];
                    assert!((again[2] - (guess + 40.0)).abs() < 1e-3, "{again:?}");
                    i += 1;
                    misses += 1;
                    guess
                }
            };
            assert!(
                (p[2] - (want - origin[2]) / UNITS).abs() < 1e-4,
                "{p:?} {from:?}"
            );
            assert!((p[0] - (from[0] - origin[0]) / UNITS).abs() < 1e-3);
        }
        assert_eq!(froms.len(), GROUND_N * GROUND_N + misses);
        assert!(misses > 0);
        // The bank is met where it stands over the guess by more than the old cast's 40 units.
        let bank = g
            .positions
            .iter()
            .filter(|p| p[2] > (guess_at(centre[1] + 200.0) + 45.0 - origin[2]) / UNITS)
            .count();
        assert!(bank > 0);
        // The floor nearest (flat) gives the guess, its slope taken in.
        let floors = [
            Plane {
                p: [centre[0] / UNITS, centre[1] / UNITS, 3.0],
                n: ragdoll_core::flinch::unit([0.0, -0.2, 1.0]).unwrap(),
            },
            Plane {
                p: [centre[0] / UNITS + 50.0, centre[1] / UNITS, 9.0],
                n: [0.0, 0.0, 1.0],
            },
        ];
        let z = floor_guess(&floors, [centre[0], centre[1] + UNITS], 0.0);
        assert!((z - (3.0 + 0.2) * UNITS).abs() < 0.01, "{z}");
        assert_eq!(floor_guess(&[], [0.0, 0.0], 17.0), 17.0);
    }

    /// The standing height is the animation's, never a lying one (Patrick's 15:30 log: taken
    /// again lying, "standing 22").
    #[test]
    fn the_standing_height_is_never_a_lying_one() {
        let standing = standing_height(8282.0 + 67.0, 8282.0);
        assert!((standing - 67.0).abs() < 1e-3);
        let lying = standing_height(8282.0 + 22.0, 8282.0);
        assert!(lying > 50.0, "{lying}");
    }

    /// Patrick's 9mm (25.9 N s on 81 kg, 0.32 m/s) is under the knockdown threshold at 100, a
    /// stagger; hits close together stack past it; the stack halves over the stacking half-life.
    #[test]
    fn a_light_hit_staggers_and_hits_stack_to_a_knockdown() {
        let topple = ragdoll_core::knock::topple_speed(ragdoll_core::knock::Body::Human, 1.0, 100);
        let nine = 25.9 / 81.0;
        let (pain, knock) = stack_hit(0.0, nine, 0.0, 0.5, topple);
        assert!(!knock, "a 9mm's {nine} m/s knocks down at {topple}");
        let mut p = pain;
        let mut down = false;
        for _ in 0..10 {
            let (q, k) = stack_hit(p, nine, 0.05, 0.5, topple);
            p = q;
            down |= k;
        }
        assert!(
            down,
            "ten rounds in half a second never knock down ({p} of {topple})"
        );
        let (half, _) = stack_hit(1.0, 0.0, 0.5, 0.5, topple);
        assert!((half - 0.5).abs() < 1e-4);
    }

    /// A frame's move is judged by the frame's length: 30 units in a 60th of a second is a jump,
    /// in a tenth (20 m/s at most) it is not.
    #[test]
    fn a_long_frame_may_move_further() {
        let origin = [0.0, 0.0, 0.0];
        let a = bodies_posed(ID, [0.0, 0.0, 0.0]);
        let b = bodies_posed(ID, [30.0, 0.0, 0.0]);
        assert!(pose_wrong(&b, &a, origin, 1.0 / 60.0).is_some());
        assert!(pose_wrong(&b, &a, origin, 0.1).is_none());
    }

    /// A body taken again moves at most RETAKE_MOST a part, whatever New Physics' step left in it
    /// (Patrick's 17:34 clip: a prone body thrown yards), and nothing that isn't finite.
    #[test]
    fn a_body_taken_again_is_never_thrown() {
        assert_eq!(capped([0.5, 0.0, 0.0], RETAKE_MOST), [0.5, 0.0, 0.0]);
        let fast = capped([30.0, 0.0, 40.0], RETAKE_MOST);
        assert!((ragdoll_core::flinch::length(fast) - RETAKE_MOST).abs() < 1e-4);
        assert!(fast[0] > 0.0 && fast[2] > fast[0]);
        assert_eq!(capped([f32::NAN, 0.0, 0.0], RETAKE_MOST), [0.0; 3]);
    }

    /// A 9mm's 25 N s gets under half of Delirium's own shot force, a 60 N s hit all of it, and
    /// the force stays within a tenth and three times Delirium's whatever the impulse.
    #[test]
    fn the_shot_force_follows_the_hit() {
        use delirium_bridge::agent::SHOT_FORCE;
        let nine = shot_force(25.0);
        assert!(nine < 0.5 * SHOT_FORCE && nine > 0.3 * SHOT_FORCE, "{nine}");
        assert_eq!(shot_force(SHOT_IMPULSE_REF), SHOT_FORCE);
        assert_eq!(shot_force(0.0), 0.1 * SHOT_FORCE);
        assert_eq!(shot_force(1.0e6), 3.0 * SHOT_FORCE);
        assert_eq!(shot_force(f32::NAN), SHOT_FORCE);
    }

    /// Between two steps: at 0 the first, at 1 the second, half way half way (place and turn).
    #[test]
    fn a_frame_is_drawn_between_two_steps() {
        let a = vec![Transform::from_translation(Vec3::ZERO); 21];
        let b = vec![
            Transform::from_translation(Vec3::new(1.0, 0.0, 0.0))
                .with_rotation(Quat::from_rotation_z(1.0));
            21
        ];
        assert_eq!(between(&a, &b, 0.0)[3].translation, Vec3::ZERO);
        let half = between(&a, &b, 0.5);
        assert!((half[3].translation.x - 0.5).abs() < 1e-6);
        assert!((half[3].rotation.angle_between(Quat::from_rotation_z(0.5))) < 1e-3);
        assert!(between(&a, &b, 1.0)[20].translation.distance(Vec3::X) < 1e-6);
    }

    /// A pose flung apart or away is refused (a body 25 units off in a frame, one 3 m from the
    /// pelvis, the pelvis 10 m from the take); a pose that moved a little is drawn.
    #[test]
    fn a_flung_pose_is_refused() {
        let origin = [0.0; 3];
        let a = bodies_posed(ID, [0.0; 3]);
        assert_eq!(pose_wrong(&a, &a, origin, 1.0 / 60.0), None);
        assert_eq!(
            pose_wrong(&bodies_posed(ID, [3.0, 0.0, 0.0]), &a, origin, 1.0 / 60.0),
            None
        );
        assert!(
            pose_wrong(&bodies_posed(ID, [30.0, 0.0, 0.0]), &a, origin, 1.0 / 60.0)
                .is_some_and(|w| w.contains("in a frame"))
        );
        let mut apart = a.clone();
        apart[5].1[2] += 4.0 * UNITS;
        assert!(pose_wrong(&apart, &[], origin, 1.0 / 60.0)
            .is_some_and(|w| w.contains("from the pelvis")));
        let far = bodies_posed(ID, [11.0 * UNITS, 0.0, 0.0]);
        assert!(
            pose_wrong(&far, &[], origin, 1.0 / 60.0).is_some_and(|w| w.contains("from the take"))
        );
    }

    /// Each body's hit lands on its own part, or its nearest parent's that has one.
    #[test]
    fn a_bodys_hit_lands_on_its_part() {
        let m = crate::new_physics::model();
        for (i, b) in m.bodies.iter().enumerate() {
            let p = part_of_body(i);
            if let Some(own) = delirium_bridge::rig::fnv_part(&b.node) {
                assert_eq!(p, own, "{}", b.node);
            }
            assert!(p < 21);
        }
        let head = m
            .bodies
            .iter()
            .position(|b| b.node == "Bip01 Head")
            .unwrap();
        assert_eq!(part_of_body(head), 12);
    }

    /// Around a body: its floor's square under it and the floor under the reference when none
    /// was found per body, in metres from the origin.
    #[test]
    fn the_world_mesh_is_measured_from_the_origin() {
        let origin = [700.0, -140.0, 70.0];
        let floors = vec![Plane {
            p: [10.0, -2.0, 1.0],
            n: [0.0, 0.0, 1.0],
        }];
        let m = world_mesh(
            (&floors, None),
            &[],
            &[],
            &[[700.0, -140.0, 100.0]],
            1.0,
            origin,
        );
        assert_eq!(m.triangles.len(), 2);
        assert!(
            m.positions.iter().all(|p| p[2].abs() < 1e-4),
            "{:?}",
            m.positions
        );
        let m = world_mesh((&[], None), &[], &[], &[], 1.0, origin);
        assert!(m.positions.iter().all(|p| p[2].abs() < 1e-4));
        assert_eq!(m.triangles.len(), 2);
    }
}
