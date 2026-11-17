//! The reactions run on an actor's 3D built from a node tree (`crate::rig_fixture`: a skeleton's
//! dump, or the mannequin laid out as FNV's tree): the real write, update and read-back of
//! [`apply`], [`after_draw`] and [`unwrite`], frame by frame, with the actor's AI moving it as the
//! game does (its reference and its 3D's root).

use super::*;
use crate::rig_fixture::{self, Fixture, Ground, ACTOR};

/// What a walking actor's reaction drew, frame by frame.
#[derive(Default, Debug)]
pub(crate) struct Walked {
    pub pelvis_while_walked: f32,
    pub delta: f32,
    pub stretch: f32,
    pub foot_under: f32,
    pub pop: f32,
    pub trace: Vec<String>,
}

/// A hit at `v` m/s from the front with its push (catch steps), then from `walk_from` seconds its
/// AI walks it at `speed` units a second along +X (the reference and the root moved each frame),
/// for `seconds`.
pub(crate) fn walked(
    nodes: Vec<rig_fixture::Node>,
    v: f32,
    walk_from: f32,
    speed: f32,
    seconds: f32,
) -> Walked {
    let mut f = Fixture::new(
        nodes,
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    clear();
    *WRITTEN.lock().unwrap_or_else(|p| p.into_inner()) = Vec::new();
    let root = f.at[0];
    f.animate();
    apply(&mut f.e);
    after_draw(&mut f.e);
    unwrite(&mut f.e);
    let mass = 80.0;
    let dir = [0.0, -1.0, 0.0];
    let pelvis = f.world("Bip01 Pelvis").expect("a pelvis").1;
    hit(
        &mut f.e,
        ACTOR,
        v * mass,
        mass,
        add(pelvis, [0.0, 9.0, 20.0]),
        dir,
        true,
    );
    push(&mut f.e, ACTOR, v * mass, mass, dir, 0);
    let start = Instant::now();
    let mut last = start;
    let mut out = Walked::default();
    let mut before: Option<(V3, V3)> = None;
    while start.elapsed().as_secs_f32() < seconds {
        std::thread::sleep(std::time::Duration::from_millis(16));
        let t = start.elapsed().as_secs_f32();
        let dt = last.elapsed().as_secs_f32();
        last = Instant::now();
        // The game's frame: its AI walks the reference; the root follows the reference.
        let mut reference = f.reference();
        let walking = t >= walk_from;
        if walking {
            reference = add(reference, [speed * dt, 0.0, 0.0]);
            rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, reference);
        }
        rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, reference);
        f.animate();
        apply(&mut f.e);
        let drawn_pelvis = f.world("Bip01 Pelvis").unwrap().1;
        let root_now = read3(&f.e, root + WORLD_TRANSLATION);
        if let Some((p0, r0)) = before {
            let own = sub(sub(drawn_pelvis, p0), sub(root_now, r0));
            out.pop = out.pop.max(length(own));
        }
        before = Some((drawn_pelvis, root_now));
        for foot in ["Bip01 L Foot", "Bip01 R Foot"] {
            let z = f.world(foot).unwrap().1[2];
            out.foot_under = out.foot_under.max(-z);
        }
        if let Some(r) = RIGS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .find(|r| r.actor == ACTOR)
        {
            if walking {
                out.pelvis_while_walked = out.pelvis_while_walked.max(r.pelvis_now);
            }
            out.delta = out.delta.max(r.drawn_delta);
            out.stretch = out.stretch.max(r.drawn_length);
            out.trace.push(format!(
                "{t:.3}: pelvis_offset {:.1} delta {:.2} length {:.2} feet z {:.1}/{:.1} steps {} yielded {}",
                r.pelvis_now,
                r.drawn_delta,
                r.drawn_length,
                f.world("Bip01 L Foot").unwrap().1[2],
                f.world("Bip01 R Foot").unwrap().1[2],
                r.balance.is_some(),
                r.yield_at.is_some()
            ));
        }
        after_draw(&mut f.e);
        unwrite(&mut f.e);
    }
    clear();
    out
}

/// A Ganger's 10mm hit and step, then its AI walking it at 101 units a second (Patrick's a7a2d15
/// Test Range clip: the pelvis drawn 30 units back, the feet 11 to 15 under the floor, the thigh
/// 26.6 long, then a pop): the pelvis drawn at most 8 units off while it is walked, no foot more
/// than 3 under the floor, no bone stretched more than 1, no pop over 2 units in a frame.
#[test]
fn a_walked_actor_after_its_step_neither_sinks_nor_pops() {
    let _one = super::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let w = walked(rig_fixture::mannequin(), 1.1, 0.55, 101.0, 2.2);
    let trace = w.trace.join("\n");
    assert!(
        w.pelvis_while_walked <= 8.0,
        "pelvis {:.1} off while walked:\n{trace}",
        w.pelvis_while_walked
    );
    assert!(
        w.foot_under <= 3.0,
        "a foot {:.1} under the floor:\n{trace}",
        w.foot_under
    );
    assert!(
        w.stretch <= 1.0,
        "a bone {:.2} off its length:\n{trace}",
        w.stretch
    );
    assert!(
        w.pop <= 2.0,
        "the pelvis popped {:.2} in a frame:\n{trace}",
        w.pop
    );
}

/// One frame of a hit's reaction on a tree, as drawn: seconds after the hit; the spine's top (the
/// highest `Bip01 Spine` node) turned off its animation and that turn's yaw (degrees); the most
/// the pelvis, the spine's top or the head is turned off its animation, read back from the nodes,
/// and as computed (`Rig::shown_turn`); the drawn flinch's own turn (`Rig::flinch_deg`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Drawn {
    pub t: f32,
    pub top: f32,
    pub yaw: f32,
    pub drawn: f32,
    pub computed: f32,
    pub flinch: f32,
}

/// A hit at `v` m/s along `dir` at `point` (world, over the reference) on the tree standing still
/// (the animation the tree's own pose), its frames for `seconds`.
pub(crate) fn flinched(
    nodes: Vec<rig_fixture::Node>,
    v: f32,
    dir: V3,
    point: V3,
    seconds: f32,
) -> Vec<Drawn> {
    let mut f = Fixture::new(
        nodes,
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    clear();
    *WRITTEN.lock().unwrap_or_else(|p| p.into_inner()) = Vec::new();
    let top = ["Bip01 Spine2", "Bip01 Spine1", "Bip01 Spine"]
        .into_iter()
        .find(|n| f.node(n).is_some())
        .expect("a spine");
    let checked = ["Bip01 Pelvis", top, "Bip01 Head"];
    f.animate();
    apply(&mut f.e);
    after_draw(&mut f.e);
    unwrite(&mut f.e);
    let mass = 80.0;
    hit(&mut f.e, ACTOR, v * mass, mass, point, dir, false);
    let start = Instant::now();
    let mut out = Vec::new();
    while start.elapsed().as_secs_f32() < seconds {
        std::thread::sleep(std::time::Duration::from_millis(16));
        f.animate();
        let anim: Vec<M3> = checked.iter().map(|n| f.world(n).unwrap().0).collect();
        apply(&mut f.e);
        let now: Vec<M3> = checked.iter().map(|n| f.world(n).unwrap().0).collect();
        let d = mul(&now[1], &transpose(&anim[1]));
        let rv = ragdoll_core::flinch::rotation_vector(&d);
        let drawn = now
            .iter()
            .zip(&anim)
            .map(|(a, b)| rig_fixture::degrees_between(a, b))
            .fold(0.0, f32::max);
        let (computed, flinch) = RIGS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .find(|r| r.actor == ACTOR)
            .map_or((0.0, 0.0), |r| (r.shown_turn, r.flinch_deg));
        out.push(Drawn {
            t: start.elapsed().as_secs_f32(),
            top: length(rv).to_degrees(),
            yaw: rv[2].to_degrees(),
            drawn,
            computed,
            flinch,
        });
        after_draw(&mut f.e);
        unwrite(&mut f.e);
    }
    clear();
    out
}

/// The drawn flinch on a tree: at least 0.8 times the flinch's own turn at the spine's top (6 and
/// 14 degrees for 0.32 and 0.79 m/s through the chest), a clavicle hit twisting it at least 6
/// degrees, the first frame after the hit within a degree of the animation, and no frame's drawn
/// turn more than a degree beyond the computed pose's change.
pub(crate) fn check_flinch(nodes: &[rig_fixture::Node]) {
    let chest = |n: &[rig_fixture::Node]| {
        let mut f = Fixture::new(
            n.to_vec(),
            Ground {
                collision: Some(0.0),
                navmesh: -8.0,
            },
        );
        f.animate();
        let s = f
            .world("Bip01 Spine1")
            .or_else(|| f.world("Bip01 Spine"))
            .unwrap()
            .1;
        let (l, r) = (
            f.world("Bip01 L Clavicle").unwrap().1,
            f.world("Bip01 R Clavicle").unwrap().1,
        );
        (s, l, r)
    };
    let (spine, l_clav, _) = chest(nodes);
    for (v, least) in [(0.32f32, 6.0f32), (0.79, 14.0)] {
        let s = flinched(
            nodes.to_vec(),
            v,
            [0.0, -1.0, 0.0],
            add(spine, [0.0, 8.0, 0.0]),
            0.7,
        );
        let trace: Vec<String> = s
            .iter()
            .map(|x| {
                format!(
                    "{:.3}: top {:.1} drawn {:.1} computed {:.1} flinch {:.1}",
                    x.t, x.top, x.drawn, x.computed, x.flinch
                )
            })
            .collect();
        let trace = trace.join("\n");
        let peak = s.iter().map(|x| x.top).fold(0.0, f32::max);
        let flinch = s.iter().map(|x| x.flinch).fold(0.0, f32::max);
        assert!(
            peak >= 0.8 * least,
            "{v} m/s: the spine's top drawn {peak:.1} degrees, want {least}:\n{trace}"
        );
        assert!(
            peak >= 0.8 * flinch,
            "{v} m/s: drawn {peak:.1} of the flinch's {flinch:.1}:\n{trace}"
        );
        assert!(
            s[0].drawn <= 1.0,
            "{v} m/s: the first frame {:.1} degrees:\n{trace}",
            s[0].drawn
        );
        let mut before = (0.0f32, 0.0f32);
        for x in &s {
            let beyond = ((x.drawn - before.0) - (x.computed - before.1)).abs();
            assert!(
                beyond <= 1.0,
                "{v} m/s: {beyond:.2} degrees beyond at {:.3}:\n{trace}",
                x.t
            );
            before = (x.drawn, x.computed);
        }
    }
    let s = flinched(
        nodes.to_vec(),
        0.79,
        [0.0, -1.0, 0.0],
        add(l_clav, [0.0, 3.0, 0.0]),
        0.4,
    );
    let twist = s.iter().map(|x| x.yaw.abs()).fold(0.0, f32::max);
    assert!(
        twist >= 6.0,
        "a clavicle hit twisted the spine's top {twist:.1} degrees"
    );
}

/// The flinch's checks on the mannequin laid out as FNV's tree.
#[test]
fn the_flinch_is_drawn_on_an_fnv_shaped_tree() {
    let _one = super::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    check_flinch(&rig_fixture::mannequin());
}
