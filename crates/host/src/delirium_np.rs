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

/// One actor's Delirium: its agent, its own body (its profile's) and its world, the origin its
/// places are measured from (the reference at the take), and how long it has stood balanced.
pub(crate) struct Dl {
    agent: Agent,
    _body: Body,
    world: World,
    pub origin: V3,
    pub frames: u32,
    balanced_for: f32,
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
        })
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
        self.agent
            .shot(&self.world, part_of_body(body), at, d, health)
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
        self.world.set_mesh(1, mesh)?;
        if dt > 0.0 {
            self.agent.step(&self.world, dt.min(0.25))?;
        }
        self.frames += 1;
        let parts = self.agent.parts()?;
        if parts.len() != 21 {
            return Err(format!("{} parts back", parts.len()));
        }
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
            w.clamp(0.0, 100.0) as u8
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
    (dl, tried, queue): (&mut Option<Dl>, &mut bool, &mut Vec<DlHit>),
    np: &mut ragdoll_core::np::Np,
    (reference, take_v, floor, dt, handing): (V3, V3, f32, f32, bool),
) -> Option<Vec<(M3, V3)>> {
    use crate::new_physics::note;
    let m = crate::new_physics::model();
    if handing {
        if dl.take().is_some() {
            note(format!(
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
            note(format!(
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
                note(format!(
                    "{id:08X}: [np delirium] taken: Delirium has it from its pose (its own body; \
                     its facts {facts:?})"
                ));
                *dl = Some(d);
            }
            Err(why) => {
                note(format!(
                    "{id:08X}: [np delirium] not taken ({why}): New Physics has it as before"
                ));
                queue.clear();
                return None;
            }
        }
    }
    let d = dl.as_mut()?;
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
    let centres: Vec<V3> = m.node_worlds(&np.states).iter().map(|w| w.1).collect();
    let mesh = world_mesh(
        &np.floors,
        &np.body_walls,
        &np.edges,
        &centres,
        floor / UNITS,
        d.origin,
    );
    let step = match failed {
        Some(why) => Err(why),
        None => d.frame(&mesh, dt),
    };
    let (drawn, done) = match step {
        Ok(Step::Drawn(drawn)) => (drawn, None),
        Ok(Step::Done(drawn, why)) => (drawn, Some(why)),
        Err(why) => {
            note(format!(
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
        note(format!(
            "{id:08X}: [np delirium] its pose went bad: New Physics has it from the pose it had"
        ));
        *dl = None;
        return None;
    }
    // New Physics' bodies on Delirium's pose (what reads them sees the real body).
    np.states = m.states_from_nodes(&drawn);
    if let Some(why) = done {
        note(format!(
            "{id:08X}: [np delirium] done after {} frames ({why}): New Physics goes on from its pose",
            d.frames
        ));
        if why == "it lies settled" {
            // Down as a fall leaves it: New Physics' downed controller settles and rises.
            np.balance.phase = ragdoll_core::np_balance::Phase::Falling;
            np.balance.since = 0.0;
            np.balance.fall_reason = "Delirium's fall";
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
        e.mem.set_f32(BASE + NPC_WEIGHT, 80.0);
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
