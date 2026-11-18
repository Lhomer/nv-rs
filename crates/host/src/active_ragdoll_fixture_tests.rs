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

/// How the fixture's animation moves under the walked actor ([`walked_as`]).
#[derive(Default, Clone, Copy)]
pub(crate) struct Motion {
    /// A walk cycle while its AI walks it: the thighs swinging 20 degrees either way and the
    /// calves bending with them at a step a 0.55 s, the pelvis bobbing a unit.
    pub gait: bool,
    /// The game's stagger animation under the hit (units): from the second frame after it, its
    /// first frame jumps the pelvis this far back along the hit and drops it a third of that, the
    /// spine leaning 12 degrees, then over 0.6 s back to the walk.
    pub stagger: f32,
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
    walked_as(nodes, v, walk_from, speed, seconds, Motion::default())
}

/// The node `name`'s local turned `deg` degrees about the world axis `axis` (carried into its
/// parent's frame) from its animation's, and its local moved by the world offset `by`.
fn pose_in_world(f: &mut Fixture, name: &str, axis: V3, deg: f32, by: V3) {
    let Some(node) = f.node(name) else { return };
    let up = rig_fixture::read9(
        &f.e,
        f.e.mem.u32(node + rig_fixture::PARENT) + rig_fixture::WORLD_ROTATION,
    );
    let a = deg.to_radians();
    let turn = ragdoll_core::flinch::rotation([axis[0] * a, axis[1] * a, axis[2] * a]);
    let local = rig_fixture::read9(&f.e, node + rig_fixture::LOCAL_ROTATION);
    let local = mul(&mul(&transpose(&up), &mul(&turn, &up)), &local);
    rig_fixture::write9(&mut f.e, node + rig_fixture::LOCAL_ROTATION, &local);
    let t = rig_fixture::read3(&f.e, node + rig_fixture::LOCAL_TRANSLATION);
    rig_fixture::write3(
        &mut f.e,
        node + rig_fixture::LOCAL_TRANSLATION,
        add(t, apply_m(&transpose(&up), by)),
    );
    f.update();
}

/// [`walked`] over the animation `motion`.
pub(crate) fn walked_as(
    nodes: Vec<rig_fixture::Node>,
    v: f32,
    walk_from: f32,
    speed: f32,
    seconds: f32,
    motion: Motion,
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
    let mut frame = 0u32;
    while start.elapsed().as_secs_f32() < seconds {
        frame += 1;
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
        if motion.gait && walking {
            let phase = (t - walk_from) * std::f32::consts::PI / 0.55;
            let swing = 20.0 * phase.sin();
            pose_in_world(
                &mut f,
                "Bip01 NonAccum",
                [0.0, 1.0, 0.0],
                0.0,
                [0.0, 0.0, -(phase.cos()).abs()],
            );
            pose_in_world(&mut f, "Bip01 L Thigh", [0.0, 1.0, 0.0], swing, [0.0; 3]);
            pose_in_world(&mut f, "Bip01 R Thigh", [0.0, 1.0, 0.0], -swing, [0.0; 3]);
            pose_in_world(
                &mut f,
                "Bip01 L Calf",
                [0.0, 1.0, 0.0],
                -15.0 * (1.0 + phase.cos()),
                [0.0; 3],
            );
            pose_in_world(
                &mut f,
                "Bip01 R Calf",
                [0.0, 1.0, 0.0],
                -15.0 * (1.0 - phase.cos()),
                [0.0; 3],
            );
        }
        if motion.stagger > 0.0 && frame >= 2 {
            // The stagger's first frame at once, then easing back over 0.6 s.
            let k = 1.0 - ((t - 0.032) / 0.6).clamp(0.0, 1.0);
            let back = motion.stagger * k;
            pose_in_world(
                &mut f,
                "Bip01 NonAccum",
                [1.0, 0.0, 0.0],
                0.0,
                [0.0, back, -back / 3.0],
            );
            pose_in_world(&mut f, "Bip01 Spine", [1.0, 0.0, 0.0], 12.0 * k, [0.0; 3]);
        }
        let anim_pelvis = f.world("Bip01 Pelvis").unwrap().1;
        apply(&mut f.e);
        read_drawn(&mut f.e);
        let drawn_pelvis = f.world("Bip01 Pelvis").unwrap().1;
        let root_now = read3(&f.e, root + WORLD_TRANSLATION);
        // The drawn pelvis's move in the world this frame beyond its animation's (the AI's walk
        // of the actor among it; the reaction's follow of the feet moves the root, never the
        // drawing).
        let mut frame_pop = 0.0f32;
        if let Some((p0, a0)) = before {
            let own = sub(sub(drawn_pelvis, p0), sub(anim_pelvis, a0));
            frame_pop = length(own);
            out.pop = out.pop.max(frame_pop);
        }
        let _ = (root_now, dt);
        before = Some((drawn_pelvis, anim_pelvis));
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
                "{t:.3}: pop {frame_pop:.2} pelvis_offset {:.1} delta {:.2} length {:.2} feet z {:.1}/{:.1} steps {} yielded {} jump {:.1} jumps {} rebased {} fade {:.2} spread {:.2}",
                r.pelvis_now,
                r.drawn_delta,
                r.drawn_length,
                f.world("Bip01 L Foot").unwrap().1[2],
                f.world("Bip01 R Foot").unwrap().1[2],
                r.balance.is_some(),
                r.yield_at.is_some(),
                r.jump_now,
                r.jumps,
                r.rebased,
                r.fade.as_ref().map_or(0.0, |f| f.left()),
                r.spread_now
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
    flinched_as(nodes, v, dir, point, seconds, false)
}

/// [`flinched`], `armed` (its weapon drawn, the hit below the stagger: it keeps its aim, as the
/// game's armed targets do).
pub(crate) fn flinched_as(
    nodes: Vec<rig_fixture::Node>,
    v: f32,
    dir: V3,
    point: V3,
    seconds: f32,
    armed: bool,
) -> Vec<Drawn> {
    let mut f = Fixture::new(
        nodes,
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    if armed {
        f.e.register_double(IS_WEAPON_DRAWN, |_, _| 1u32.into_ret());
    }
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
    hit(&mut f.e, ACTOR, v * mass, mass, point, dir, armed);
    let start = Instant::now();
    let mut out = Vec::new();
    while start.elapsed().as_secs_f32() < seconds {
        std::thread::sleep(std::time::Duration::from_millis(16));
        f.animate();
        let anim: Vec<M3> = checked.iter().map(|n| f.world(n).unwrap().0).collect();
        apply(&mut f.e);
        // The world pass: what the skin draws with, read as the game's frame reads it.
        read_drawn(&mut f.e);
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
        after_draw(&mut f.e);
        let skin_turn = RIGS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .find(|r| r.actor == ACTOR)
            .map_or(0.0, |r| r.drawn_turn);
        assert!(
            (skin_turn - drawn).abs() <= 0.05,
            "the skin's read ({skin_turn:.2}) is not the nodes' ({drawn:.2})"
        );
        out.push(Drawn {
            t: start.elapsed().as_secs_f32(),
            top: length(rv).to_degrees(),
            yaw: rv[2].to_degrees(),
            drawn,
            computed,
            flinch,
        });
        unwrite(&mut f.e);
    }
    clear();
    out
}

/// The drawn flinch on a tree: at least 0.8 times the flinch's own turn at the spine's top (6, 14
/// and 25 degrees for 0.32, 0.79 and 2 m/s through the chest), a clavicle hit twisting it at least 6
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
    for (v, least) in [(0.32f32, 6.0f32), (0.79, 14.0), (2.0, 25.0)] {
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
        let back = s
            .iter()
            .filter(|x| x.top >= 2.0)
            .map(|x| x.t)
            .fold(0.0, f32::max);
        let beyond_most = s
            .windows(2)
            .map(|w| ((w[1].drawn - w[0].drawn) - (w[1].computed - w[0].computed)).abs())
            .fold(0.0, f32::max);
        eprintln!(
            "RESULT flinch {v} m/s: spine top peak {peak:.1} deg (flinch {flinch:.1}, {:.0}%), first frame {:.2} deg, drawn vs computed per frame at most {beyond_most:.2} deg, under 2 deg after {back:.2} s",
            100.0 * peak / flinch.max(1e-3),
            s[0].drawn
        );
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
    eprintln!("RESULT clavicle twist {twist:.1} deg");
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
    let _layer = super::tests::layer_on();
    check_flinch(&rig_fixture::mannequin());
}

/// The flinch's checks on FNV's own skeleton (`rig_fixture::fnv_skeleton`).
#[test]
fn the_flinch_is_drawn_on_fnvs_skeleton() {
    let _one = super::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let _layer = super::tests::layer_on();
    check_flinch(&rig_fixture::fnv_skeleton());
}

/// The walked Ganger on FNV's own skeleton.
#[test]
fn a_walked_actor_neither_sinks_nor_pops_on_fnvs_skeleton() {
    let _one = super::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let w = walked(rig_fixture::fnv_skeleton(), 1.1, 0.55, 101.0, 2.2);
    eprintln!(
        "RESULT walked: pelvis off while walked {:.1}, foot under the floor {:.1}, drawn_length {:.2}, drawn_delta {:.2}, pelvis pop {:.2} units/frame",
        w.pelvis_while_walked, w.foot_under, w.stretch, w.delta, w.pop
    );
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

/// The Test Range's walking Ganger hit and staggered (Patrick's bdf710a review, clip 1456 at
/// 34.3-34.5 s: `drawn_delta` 4.5 units on the stagger hits): on FNV's skeleton, walking with a
/// walk cycle, the game's stagger animation jumping its pelvis 8 units back under the hit, the
/// drawn pelvis and feet slide at most 2 units a frame (`drawn_delta`), no pop over 2, no foot
/// more than 3 under the floor, no bone stretched past 1.
#[test]
fn a_walking_ganger_staggered_on_fnvs_skeleton() {
    let _one = super::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let w = walked_as(
        rig_fixture::fnv_skeleton(),
        1.1,
        0.0,
        101.0,
        2.0,
        Motion {
            gait: true,
            stagger: 8.0,
        },
    );
    eprintln!(
        "RESULT walking Ganger staggered: pelvis off while walked {:.1}, foot under the floor {:.1}, drawn_length {:.2}, drawn_delta {:.2}, pelvis pop beyond the animation {:.2} units/frame",
        w.pelvis_while_walked, w.foot_under, w.stretch, w.delta, w.pop
    );
    let trace = w.trace.join("\n");
    assert!(w.delta <= 2.0, "drawn_delta {:.2}:\n{trace}", w.delta);
    assert!(
        w.pop <= 2.0,
        "the pelvis popped {:.2} in a frame:\n{trace}",
        w.pop
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
}

/// [`flinched`] with the game's own hit animation starting under the hit: from the second frame
/// after it, the spine's root turned `jump` degrees back at once (a new animation group's first
/// frame), held. The spine's top read against that animation.
pub(crate) fn flinched_over_a_jump(
    nodes: Vec<rig_fixture::Node>,
    v: f32,
    jump: f32,
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
    let spine = f.node("Bip01 Spine").expect("a spine");
    let spine_i = f.at.iter().position(|&a| a == spine).unwrap();
    let rest = f.nodes[spine_i].r;
    let top = f.node("Bip01 Spine2").expect("a spine top");
    f.animate();
    apply(&mut f.e);
    after_draw(&mut f.e);
    unwrite(&mut f.e);
    let chest = f.world("Bip01 Spine1").unwrap().1;
    let mass = 80.0;
    hit(
        &mut f.e,
        ACTOR,
        v * mass,
        mass,
        add(chest, [0.0, 8.0, 0.0]),
        [0.0, -1.0, 0.0],
        false,
    );
    let start = Instant::now();
    let mut out = Vec::new();
    let mut frame = 0;
    while start.elapsed().as_secs_f32() < seconds {
        std::thread::sleep(std::time::Duration::from_millis(16));
        frame += 1;
        f.animate();
        if frame >= 2 {
            // Back about the world's side axis, carried into the spine's parent frame.
            let up = rig_fixture::read9(
                &f.e,
                f.e.mem.u32(spine + rig_fixture::PARENT) + rig_fixture::WORLD_ROTATION,
            );
            let turn = ragdoll_core::flinch::rotation([-jump.to_radians(), 0.0, 0.0]);
            let local = mul(&mul(&transpose(&up), &mul(&turn, &up)), &rest);
            rig_fixture::write9(&mut f.e, spine + rig_fixture::LOCAL_ROTATION, &local);
            f.update();
        }
        let anim = rig_fixture::read9(&f.e, top + rig_fixture::WORLD_ROTATION);
        apply(&mut f.e);
        let now = rig_fixture::read9(&f.e, top + rig_fixture::WORLD_ROTATION);
        let rv = ragdoll_core::flinch::rotation_vector(&mul(&now, &transpose(&anim)));
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
            drawn: length(rv).to_degrees(),
            computed,
            flinch,
        });
        after_draw(&mut f.e);
        unwrite(&mut f.e);
    }
    clear();
    out
}

/// The game's hit animation starting under the hit (a 30 degree jump of the spine in a frame, as
/// a new animation group's first frame) does not take the drawn flinch away: on FNV's skeleton the
/// spine's top is still drawn at least 0.8 times the flinch's 14 degrees (0.79 m/s) over it.
#[test]
fn the_flinch_survives_the_games_hit_animation_on_fnvs_skeleton() {
    let _one = super::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let _layer = super::tests::layer_on();
    let s = flinched_over_a_jump(rig_fixture::fnv_skeleton(), 0.79, 30.0, 0.7);
    let trace: Vec<String> = s
        .iter()
        .map(|x| {
            format!(
                "{:.3}: top {:.1} computed {:.1} flinch {:.1}",
                x.t, x.top, x.computed, x.flinch
            )
        })
        .collect();
    let trace = trace.join("\n");
    let peak = s.iter().map(|x| x.top).fold(0.0, f32::max);
    assert!(
        peak >= 0.8 * 14.0,
        "drawn {peak:.1} over the game's hit animation:\n{trace}"
    );
}

/// An armed actor hit below the stagger (it keeps its aim: the game's Gangers and the tester's
/// target) on FNV's skeleton: the spine's top still drawn at least 0.8 times 6 and 14 degrees.
#[test]
fn an_armed_actor_flinches_on_fnvs_skeleton() {
    let _one = super::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let _layer = super::tests::layer_on();
    let nodes = rig_fixture::fnv_skeleton();
    let mut f = Fixture::new(
        nodes.clone(),
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    f.animate();
    let chest = f.world("Bip01 Spine1").unwrap().1;
    drop(f);
    for (v, least) in [(0.32f32, 6.0f32), (0.79, 14.0)] {
        let s = flinched_as(
            nodes.clone(),
            v,
            [0.0, -1.0, 0.0],
            add(chest, [0.0, 8.0, 0.0]),
            0.6,
            true,
        );
        let trace: Vec<String> = s
            .iter()
            .map(|x| {
                format!(
                    "{:.3}: top {:.1} computed {:.1} flinch {:.1}",
                    x.t, x.top, x.computed, x.flinch
                )
            })
            .collect();
        let peak = s.iter().map(|x| x.top).fold(0.0, f32::max);
        assert!(
            peak >= 0.8 * least,
            "{v} m/s armed: drawn {peak:.1}:\n{}",
            trace.join("\n")
        );
    }
}

/// MOCKED update (not the game's): the update leaves `Bip01 Spine` and everything under it as it
/// was (as a node the game's update skips would be). The write check sets those worlds itself:
/// on FNV's skeleton the spine's top is still drawn at least 0.8 times 14 degrees for 0.79 m/s.
#[test]
fn the_write_reaches_the_spine_when_the_update_skips_it_on_fnvs_skeleton() {
    let _one = super::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let _layer = super::tests::layer_on();
    let nodes = rig_fixture::fnv_skeleton();
    let mut f = Fixture::new(
        nodes.clone(),
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    f.animate();
    let chest = f.world("Bip01 Spine1").unwrap().1;
    let spine = f.node("Bip01 Spine").unwrap();
    // The update with the spine's subtree left out.
    let skipped: Vec<u32> =
        f.at.iter()
            .copied()
            .filter(|&n| {
                let mut a = n;
                for _ in 0..16 {
                    if a == spine {
                        return true;
                    }
                    a = f.e.mem.u32(a + rig_fixture::PARENT);
                    if a == 0 {
                        break;
                    }
                }
                false
            })
            .collect();
    let order: Vec<u32> =
        f.at.iter()
            .copied()
            .filter(|n| !skipped.contains(n))
            .collect();
    clear();
    *WRITTEN.lock().unwrap_or_else(|p| p.into_inner()) = Vec::new();
    f.e.register_double(0x00a5_9c60, move |e, _| {
        rig_fixture::update_worlds(e, &order);
        0u32.into_ret()
    });
    let top = f.node("Bip01 Spine2").unwrap();
    apply(&mut f.e);
    after_draw(&mut f.e);
    unwrite(&mut f.e);
    hit(
        &mut f.e,
        ACTOR,
        0.79 * 80.0,
        80.0,
        add(chest, [0.0, 8.0, 0.0]),
        [0.0, -1.0, 0.0],
        false,
    );
    let start = Instant::now();
    let mut peak = 0.0f32;
    while start.elapsed().as_secs_f32() < 0.5 {
        std::thread::sleep(std::time::Duration::from_millis(16));
        f.animate();
        let anim = rig_fixture::read9(&f.e, top + rig_fixture::WORLD_ROTATION);
        apply(&mut f.e);
        let now = rig_fixture::read9(&f.e, top + rig_fixture::WORLD_ROTATION);
        peak = peak.max(rig_fixture::degrees_between(&now, &anim));
        after_draw(&mut f.e);
        unwrite(&mut f.e);
        f.animate();
    }
    clear();
    assert!(
        peak >= 0.8 * 14.0,
        "the spine's top drawn {peak:.1} degrees"
    );
}
