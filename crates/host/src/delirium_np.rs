//! New Physics' actors on Delirium (`delirium_bridge`), where it runs (the player's own GTA IV
//! named in `sDeliriumGame`, its start logged): a human New Physics takes on a hit is given to a
//! Delirium agent made from its pose, with its own profile, the shot given to it, and each frame
//! its parts posed onto FNV's bones (`delirium_bridge::rig`) become the bodies New Physics draws;
//! New Physics' own bodies are kept on that pose, so what reads them sees the real body, and once
//! the agent lies settled (or has stood balanced a while) New Physics' own controller goes on from
//! that pose: its settle and rise (the arms pushing through planted hands), its death hand-over.
//! Anything Delirium refuses lets New Physics take the actor at once, said in the log.
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
pub(crate) fn bones_from_parts(rig: &Rig, parts: &[Transform]) -> Vec<Transform> {
    let s = skel();
    let (local, hips) = rig.pose(parts);
    let n = s.names.len();
    let mut w = vec![Transform::IDENTITY; n];
    for b in 0..n {
        w[b] = match s.parents[b] {
            None => Transform::from_translation(s.local_t[b]).with_rotation(local[b]),
            Some(p) => Transform::from_translation(w[p].translation + w[p].rotation * s.local_t[b])
                .with_rotation((w[p].rotation * local[b]).normalize()),
        };
    }
    let shift = hips - w[rig.hips()].translation;
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
    floors: &[Plane],
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
    for (b, c) in centres.iter().enumerate() {
        let c = local([c[0] / UNITS, c[1] / UNITS, c[2] / UNITS]);
        if let Some(pl) = floors.get(b) {
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
    if floors.is_empty() {
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

/// One actor's Delirium: its agent, its own body (its profile's) and its world, the origin its
/// places are measured from (the reference at the take), and how long it has stood balanced.
pub(crate) struct Dl {
    agent: Agent,
    _body: Body,
    world: World,
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
    /// units), moving as `velocity` (m/s), at `origin`.
    pub(crate) fn take(
        bone: &dyn Fn(&str) -> Option<(M3, V3)>,
        velocity: [f32; 3],
        origin: V3,
        profile: &delirium_bridge::profile::Profile,
    ) -> Result<Dl, String> {
        let st = delirium_bridge::started().ok_or("Delirium isn't running")?;
        let s = skel();
        let mut worlds = s.rest.clone();
        for (i, n) in s.names.iter().enumerate() {
            if delirium_bridge::rig::fnv_part(n).is_some() {
                let (r, t) = bone(n).ok_or_else(|| format!("no {n} in the actor's tree"))?;
                worlds[i] = to_tf(&r, t, origin);
            }
        }
        let parts = st.rig.parts_at(&worlds);
        let body = Body::new(&st.build, &st.frames, profile.kg)?;
        let v = Vec3::from_array(velocity);
        let states: Vec<PartState> = parts
            .iter()
            .map(|&frame| PartState {
                frame,
                velocity: v,
                spin: Vec3::ZERO,
            })
            .collect();
        let agent = Agent::new(&body, &states, profile)?;
        Ok(Dl {
            agent,
            _body: body,
            world: World::new()?,
            origin,
            frames: 0,
            balanced_for: 0.0,
            standing: 0.0,
            last: Vec::new(),
            pending: 0.0,
            steps: (Vec::new(), Vec::new()),
            corpse: None,
        })
    }

    /// Dead: its muscles off (Delirium's corpse), its ragdoll kept on the pose drawn when the
    /// game's death would turn it loose. Answers whether it is held (an actor with no ragdoll
    /// controller is not).
    pub(crate) fn die(&mut self, e: &engine::prelude::Engine, actor: u32) -> Result<bool, String> {
        if self.corpse.is_none() {
            self.agent.kill()?;
            self.corpse = CorpseHold::of(e, actor);
        }
        Ok(self.corpse.is_some())
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
        self.agent.shot_force(
            &self.world,
            part_of_body(body),
            at,
            d,
            health,
            shot_force(p),
        )
    }

    /// A blast at `centre` (game units), reaching `radius` (game units), `speed` m/s at its centre.
    pub(crate) fn blast(&mut self, centre: V3, radius: f32, speed: f32) -> Result<(), String> {
        let at = Vec3::new(
            (centre[0] - self.origin[0]) / UNITS,
            (centre[1] - self.origin[1]) / UNITS,
            (centre[2] - self.origin[2]) / UNITS,
        );
        self.agent.explosion(&self.world, at, radius / UNITS, speed)
    }

    /// One frame: the world set to `mesh`, the agent stepped by `dt`, its pose as New Physics'
    /// bodies; done once it lies settled half a second (or stood balanced two seconds).
    pub(crate) fn frame(
        &mut self,
        mesh: &delirium_bridge::world::Mesh,
        dt: f32,
    ) -> Result<Step, String> {
        let st = delirium_bridge::started().ok_or("Delirium isn't running")?;
        use delirium_bridge::agent::STEP;
        self.world.set_mesh(1, mesh)?;
        if self.steps.1.is_empty() {
            let now = self.agent.parts()?;
            self.steps = (now.clone(), now);
        }
        // Whole steps of Delirium's own (at most four a frame: a long frame is not caught up).
        self.pending += dt.clamp(0.0, 4.0 * STEP);
        while self.pending >= STEP - 1e-6 {
            self.agent.step(&self.world, STEP)?;
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
        let drawn = bodies_from_parts(&st.rig, &parts, self.origin);
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
}

impl Posture {
    /// Down: the pelvis under 55% of its standing height, or the torso more than 60 degrees from
    /// upright (fallen over, lying, seated on the ground).
    pub(crate) fn downed(&self, standing: f32) -> bool {
        self.pelvis_up < 0.55 * standing || self.lean > 60.0
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
    Posture {
        pelvis_up: pelvis[2] - floor,
        lean,
        speed: length([v[0], v[1], v[2]]),
        outside,
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

/// Why a pose of Delirium's is not to be drawn (none: it may be): a body that moved more than
/// 25 units since the last frame, one more than 3 m from the pelvis (a body flung apart), or the
/// pelvis more than 10 m from where the take began (`origin`). Patrick's 22:38 clip: two bodies
/// vanished a few frames after the take, as if flung out of the world.
pub(crate) fn pose_wrong(drawn: &[(M3, V3)], last: &[(M3, V3)], origin: V3) -> Option<String> {
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
            if moved > 25.0 {
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
    if !delirium_bridge::running() {
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
    if !delirium_bridge::running() {
        return;
    }
    if matches!(queue.last(), Some(DlHit::Shot(..))) {
        queue.pop();
    }
    queue.push(DlHit::Blast(centre, radius, speed));
}

/// The actor's frame on Delirium, where it runs: its Delirium made at its first frame (once a
/// take: its bones read from the animation's snapshot), the queued hits given, the world New
/// Physics found around it set, a step; New Physics' bodies (`np`) put on its pose, which is
/// answered as the frame's drawn bodies. None: New Physics' own frame (never taken, refused, or
/// done: New Physics goes on from the pose Delirium left; dying: New Physics hands it over, its
/// wait kept and the limbs brought in).
#[allow(clippy::too_many_arguments)]
pub(crate) fn frame_for(
    e: &mut engine::prelude::Engine,
    (id, actor, tree, snap): (
        u32,
        u32,
        &[(u32, u32)],
        Option<&crate::new_physics::Snapshot>,
    ),
    (dl, tried, queue, down): (&mut Option<Dl>, &mut bool, &mut Vec<DlHit>, &mut bool),
    np: &mut ragdoll_core::np::Np,
    (reference, take_v, floor, dt, handing): (V3, V3, f32, f32, bool),
) -> Option<Vec<(M3, V3)>> {
    use crate::new_physics::{note, note_event};
    let m = crate::new_physics::model();
    if handing {
        if dl.take().is_some() {
            note_event(format!(
                "{id:08X}: [np delirium] handed over (dying or given up): New Physics hands it \
                 over from Delirium's pose"
            ));
        }
        queue.clear();
        return None;
    }
    if dl.is_none() && !*tried {
        *tried = true;
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
        // Each NPC its own settings (`delirium_bridge::profile`: neutral for now) from its facts.
        let facts = facts_of(e, actor);
        if !delirium_bridge::profile::takes(&facts) {
            note_event(format!(
                "{id:08X}: [np delirium] not taken (not a human NPC): New Physics has it as before"
            ));
            queue.clear();
            return None;
        }
        let v = [take_v[0] / UNITS, take_v[1] / UNITS, take_v[2] / UNITS];
        match Dl::take(
            &bone,
            v,
            reference,
            &delirium_bridge::profile::profile(&facts),
        ) {
            Ok(d) => {
                note_event(format!(
                    "{id:08X}: [np delirium] taken: Delirium has it from its pose (its own body; \
                     its facts {facts:?})"
                ));
                *dl = Some(d);
            }
            Err(why) => {
                note_event(format!(
                    "{id:08X}: [np delirium] not taken ({why}): New Physics has it as before"
                ));
                queue.clear();
                return None;
            }
        }
    }
    let Some(d) = dl.as_mut() else {
        queue.clear();
        return None;
    };
    let mut failed = None;
    let health = if queue.is_empty() {
        1.0
    } else {
        health_left(e, actor)
    };
    for h in queue.drain(..) {
        let r = match h {
            DlHit::Shot(b, point, impulse) => d.shot(b, point, impulse, health),
            DlHit::Blast(c, r, speed) => d.blast(c, r, speed),
        };
        if let Err(why) = r {
            failed = Some(why);
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
    let mesh = world_mesh(
        floors,
        &np.body_walls,
        &np.edges,
        &centres,
        floor / UNITS,
        d.origin,
    );
    if d.frames == 0 {
        d.standing = centres.get(m.root).map_or(0.0, |c| c[2] - floor).max(1.0);
    }
    let step = match failed {
        Some(why) => Err(why),
        None => d.frame(&mesh, dt),
    };
    let (drawn, done) = match step {
        Ok(Step::Drawn(drawn)) => (drawn, None),
        Ok(Step::Done(drawn, why)) => (drawn, Some(why)),
        Err(why) => {
            note_event(format!(
                "{id:08X}: [np delirium] refused ({why}): New Physics has it from the pose it had"
            ));
            *dl = None;
            return None;
        }
    };
    if !drawn
        .iter()
        .all(|(r, t)| r.iter().all(|x| x.is_finite()) && t.iter().all(|x| x.is_finite()))
    {
        note_event(format!(
            "{id:08X}: [np delirium] its pose went bad: New Physics has it from the pose it had"
        ));
        *dl = None;
        return None;
    }
    // Never a body flung away: New Physics takes it from the last pose that was right.
    if let Some(why) = pose_wrong(&drawn, &d.last, d.origin) {
        note_event(format!(
            "{id:08X}: [np delirium] refused (its pose went wrong: {why}): New Physics has it \
             from the last right pose"
        ));
        *dl = None;
        return None;
    }
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
    // New Physics' bodies on Delirium's pose (what reads them sees the real body), moving as it
    // moves.
    let moving = velocities(&drawn, &d.last, dt);
    put_on(np, &drawn, &moving);
    d.last = drawn.clone();
    // Down (fallen over, on the ground): New Physics' downed controller has it from here, moving
    // as Delirium had it: its ground, its hits on a limp body, its settle and its get-up (the arms
    // pushing off through planted hands; Patrick's choice: never GTA IV's get-ups). Not a corpse.
    let done = match done {
        None if !d.dead() && pose.downed(d.standing) => Some("downed"),
        other => other,
    };
    if done == Some("downed") {
        note_event(format!(
            "{id:08X}: [np delirium] downed after {} frames (the pelvis {:.0} u over the floor of \
             {:.0} standing, the torso {:.0} degrees from upright, {:.2} m/s): New Physics' downed \
             controller has it from Delirium's pose, its settle and get-up",
            d.frames, pose.pelvis_up, d.standing, pose.lean, pose.speed
        ));
        np.balance.phase = ragdoll_core::np_balance::Phase::Falling;
        np.balance.since = 0.0;
        np.balance.fall_reason = "downed on Delirium";
        *dl = None;
        *down = true;
        return Some(drawn);
    }
    if let Some(why) = done {
        note_event(format!(
            "{id:08X}: [np delirium] done after {} frames ({why}): New Physics goes on from its pose",
            d.frames
        ));
        if why == "it lies settled" {
            // Down as a fall leaves it: New Physics' downed controller settles and rises.
            np.balance.phase = ragdoll_core::np_balance::Phase::Falling;
            np.balance.since = 0.0;
            np.balance.fall_reason = "Delirium's fall";
            *down = true;
        }
        *dl = None;
    }
    Some(drawn)
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
        assert_eq!(pose_wrong(&a, &a, origin), None);
        assert_eq!(
            pose_wrong(&bodies_posed(ID, [3.0, 0.0, 0.0]), &a, origin),
            None
        );
        assert!(pose_wrong(&bodies_posed(ID, [30.0, 0.0, 0.0]), &a, origin)
            .is_some_and(|w| w.contains("in a frame")));
        let mut apart = a.clone();
        apart[5].1[2] += 4.0 * UNITS;
        assert!(pose_wrong(&apart, &[], origin).is_some_and(|w| w.contains("from the pelvis")));
        let far = bodies_posed(ID, [11.0 * UNITS, 0.0, 0.0]);
        assert!(pose_wrong(&far, &[], origin).is_some_and(|w| w.contains("from the take")));
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
        let m = world_mesh(&floors, &[], &[], &[[700.0, -140.0, 100.0]], 1.0, origin);
        assert_eq!(m.triangles.len(), 2);
        assert!(
            m.positions.iter().all(|p| p[2].abs() < 1e-4),
            "{:?}",
            m.positions
        );
        let m = world_mesh(&[], &[], &[], &[], 1.0, origin);
        assert!(m.positions.iter().all(|p| p[2].abs() < 1e-4));
        assert_eq!(m.triangles.len(), 2);
    }
}
