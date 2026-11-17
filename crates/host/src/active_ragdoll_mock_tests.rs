//! The host's reaction run on a mock engine (Patrick's a45deed clips: the drawn flinch never
//! reached the screen while the lab, which copies the pipeline, passed): a biped's node tree in
//! the engine's memory (the 20 `Bip01` bones of `ragdoll_core::flinch::BONES`, FNV's spine under
//! `Bip01 NonAccum`), the game's calls `apply` and `hit` make answered by doubles, and the frames
//! run as the game runs them: [`hit`], then each frame [`apply`] (the write and the update of the
//! world transforms), the drawn pose read from the nodes, [`after_draw`] and [`unwrite`].

use super::tests::SERIAL;
use super::*;

/// Where the mock lays out its objects.
const BASE: u32 = 0x0100_0000;
const NODE_SIZE: u32 = 0x100;
const ROOT: u32 = BASE;
const NON_ACCUM: u32 = BASE + NODE_SIZE;
const FIRST_BONE: u32 = BASE + 2 * NODE_SIZE;
const NAMES: u32 = BASE + 0x4000;
const ACTOR: u32 = BASE + 0x6000;
const PROCESS_AT: u32 = BASE + 0x7000;
const NODE_VTABLE: u32 = BASE + 0x8000;
const PROCESS_VTABLE: u32 = BASE + 0x9000;
const ACTOR_ID: u32 = 0x0012_3456;
/// The doubles' own addresses (no game code there).
const IS_SELF: u32 = 0x7100_0000;
const NO_ACTION: u32 = 0x7100_0010;

/// Each rig bone's joint at rest over the actor's reference (game units; the lab's mannequin,
/// facing +Y, its left at -X), in [`BONES`]'s order.
const JOINT: [V3; 20] = [
    [0.0, 0.0, 66.0],
    [0.0, 0.0, 70.0],
    [0.0, 0.0, 78.0],
    [0.0, 0.0, 86.0],
    [0.0, 0.0, 102.0],
    [0.0, 0.5, 108.0],
    [-2.0, 0.0, 100.0],
    [2.0, 0.0, 100.0],
    [-13.0, 0.0, 100.0],
    [13.0, 0.0, 100.0],
    [-14.0, 0.5, 79.0],
    [14.0, 0.5, 79.0],
    [-7.0, 0.0, 64.0],
    [7.0, 0.0, 64.0],
    [-7.0, 1.0, 35.0],
    [7.0, 1.0, 35.0],
    [-7.0, 0.0, 6.0],
    [7.0, 0.0, 6.0],
    [-15.0, 1.0, 59.0],
    [15.0, 1.0, 59.0],
];

/// Each bone's node parent (an index into [`BONES`]; `None`: `Bip01 NonAccum`).
const NODE_PARENT: [Option<usize>; 20] = [
    None,
    None,
    Some(1),
    Some(2),
    Some(3),
    Some(4),
    Some(3),
    Some(3),
    Some(6),
    Some(7),
    Some(8),
    Some(9),
    Some(0),
    Some(0),
    Some(12),
    Some(13),
    Some(14),
    Some(15),
    Some(10),
    Some(11),
];

const SPINE_TOP: usize = 3;

/// Each bone's world rotation at rest: as FNV's skeleton has them, none the identity (a bone's
/// own X along it, its axes turned about it), so a local and a world turn taken for each other
/// show. NonAccum's too.
fn rest(bone: Option<usize>) -> M3 {
    let k = bone.map_or(0.37, |b| b as f32 * 0.61 + 0.2);
    let along =
        ragdoll_core::flinch::rotation([0.0, -std::f32::consts::FRAC_PI_2 + 0.3 * k.sin(), 0.0]);
    let about = ragdoll_core::flinch::rotation([0.7 * k.cos(), 0.0, 0.5 * (2.0 * k).sin()]);
    mul(&about, &along)
}

fn node_of(bone: usize) -> u32 {
    FIRST_BONE + bone as u32 * NODE_SIZE
}

/// Every node with its parent (0 for the root), the tree's order: parents first.
fn tree() -> Vec<(u32, u32)> {
    let mut t = vec![(ROOT, 0), (NON_ACCUM, ROOT)];
    for (b, p) in NODE_PARENT.iter().enumerate() {
        t.push((node_of(b), p.map_or(NON_ACCUM, node_of)));
    }
    t
}

/// The worlds of every node from their locals, parents first (`NiAVObject::UpdateDownwardPass`
/// with no controllers).
fn update_worlds(e: &mut Engine) {
    for (node, parent) in tree() {
        let local = pose::Xf {
            r: read9(e, node + LOCAL_ROTATION),
            t: read3(e, node + LOCAL_TRANSLATION),
            s: e.mem.f32(node + LOCAL_SCALE),
        };
        let world = if parent == 0 {
            local
        } else {
            read_world(e, parent).then(&local)
        };
        write9(e, node + WORLD_ROTATION, &world.r);
        write3(e, node + WORLD_TRANSLATION, world.t);
        e.mem.set_f32(node + WORLD_SCALE, world.s);
    }
}

/// The engine with the actor and its standing biped at rest (the animation: every local its rest).
fn mock() -> Engine {
    let mut e = Engine::new();
    e.mem.map(BASE, 0x10000);
    // The node tree: names, parents, rest locals (identity rotations).
    let mut name_at = NAMES;
    let mut put_name = |e: &mut Engine, node: u32, name: &str| {
        e.mem.set_cstr(name_at, name.as_bytes());
        e.mem.set_u32(node + NODE_NAME, name_at);
        name_at += 0x20;
    };
    put_name(&mut e, ROOT, "Scene Root");
    put_name(&mut e, NON_ACCUM, "Bip01 NonAccum");
    for (b, (name, _)) in BONES.iter().enumerate() {
        put_name(&mut e, node_of(b), name);
    }
    for (node, parent) in tree() {
        e.mem.set_u32(node, NODE_VTABLE);
        e.mem.set_u32(node + PARENT, parent);
        let (rot, at) = if node == ROOT {
            (IDENTITY, [0.0; 3])
        } else if node == NON_ACCUM {
            (rest(None), [0.0, 0.0, 66.0])
        } else {
            let b = ((node - FIRST_BONE) / NODE_SIZE) as usize;
            let (up_r, up_at) = NODE_PARENT[b].map_or((rest(None), [0.0, 0.0, 66.0]), |p| {
                (rest(Some(p)), JOINT[p])
            });
            (
                mul(&transpose(&up_r), &rest(Some(b))),
                apply_m(&transpose(&up_r), sub(JOINT[b], up_at)),
            )
        };
        write9(&mut e, node + LOCAL_ROTATION, &rot);
        write3(&mut e, node + LOCAL_TRANSLATION, at);
        e.mem.set_f32(node + LOCAL_SCALE, 1.0);
    }
    update_worlds(&mut e);
    // The node's `IsNode` (itself), its children, the update, the actor's 3D and form.
    e.register_double(IS_SELF, |_, a| a[0].into_ret());
    e.mem.set_u32(NODE_VTABLE + IS_NODE_SLOT, IS_SELF);
    e.mem.set_u32(NODE_VTABLE + IS_GEOMETRY_SLOT, NO_ACTION);
    let kids = |node: u32| -> Vec<u32> {
        tree()
            .into_iter()
            .filter(|&(_, p)| p == node)
            .map(|(n, _)| n)
            .collect()
    };
    e.register_double(CHILD_COUNT, move |_, a| {
        (kids(a[0]).len() as u32).into_ret()
    });
    e.register_double(CHILD_AT, move |_, a| {
        kids(a[0])
            .get(a[1] as usize)
            .copied()
            .unwrap_or(0)
            .into_ret()
    });
    e.register_double(UPDATE_DATA, |_, a| a[0].into_ret());
    e.register_double(NODE_UPDATE, |e, _| {
        update_worlds(e);
        0u32.into_ret()
    });
    e.register_double(ACTOR_3D, |_, a| {
        if a[0] == ACTOR { ROOT } else { 0 }.into_ret()
    });
    e.register_double(LOOKUP_FORM, |_, a| {
        if a[0] == ACTOR_ID { ACTOR } else { 0 }.into_ret()
    });
    e.register_double(GET_SCALE, |_, _| 1.0f32.into_ret());
    e.register_double(IS_WEAPON_DRAWN, |_, _| 0u32.into_ret());
    e.register_double(NO_ACTION, |_, _| 0u32.into_ret());
    // The actor: its form id, its process (middle-high, standing, its action none), alive.
    e.mem.set_u32(ACTOR + FORM_ID, ACTOR_ID);
    e.mem.set_u32(ACTOR + PROCESS, PROCESS_AT);
    e.mem.set_u32(ACTOR + LIFE_STATE, 0);
    write3(&mut e, ACTOR + REFR_POSITION, [0.0; 3]);
    e.mem.set_u32(PROCESS_AT, PROCESS_VTABLE);
    e.mem.set_u8(PROCESS_AT + PROCESS_LEVEL, 0);
    e.mem.set_u8(PROCESS_AT + KNOCK_STATE, 0);
    e.mem.set_u32(PROCESS_VTABLE + 0x3e4, NO_ACTION);
    e
}

/// One frame sampled: seconds after the hit, the spine's top drawn off its animation (its world
/// rotation against the rest's identity; degrees) and that turn's yaw about the vertical (degrees).
#[derive(Clone, Copy, Debug)]
struct Sample {
    t: f32,
    turn: f32,
    yaw: f32,
    /// The most the pelvis, the spine's top or the head is drawn off its animation (the nodes
    /// read back, degrees), and the same as the reaction computed it (`Rig::shown_turn`).
    drawn: f32,
    computed: f32,
    /// The pose log's `drawn_delta` and `drawn_length` this frame.
    delta: f32,
    stretch: f32,
}

/// The pelvis, the spine's top and the head: the bones `drawn_turn` is read on.
const CHECKED: [usize; 3] = [0, SPINE_TOP, 5];

/// A hit pushing `v` m/s along `dir` at `point`, then the frames as the game draws them for
/// `seconds` (about 60 a second): each frame's spine top as written and updated, read back.
fn run(v: f32, dir: V3, point: V3, seconds: f32) -> Vec<Sample> {
    run_with(v, dir, point, seconds, |_| None)
}

/// [`run`] with the game's own animation `anim` playing from the hit: at `t` seconds after it,
/// the extra turn (a rotation vector, world, rad) of the spine's bones on top of their rest, as
/// the game's animation update writes their locals before the frame's draw (a stagger).
fn run_with(
    v: f32,
    dir: V3,
    point: V3,
    seconds: f32,
    anim: impl Fn(f32) -> Option<V3>,
) -> Vec<Sample> {
    let mut e = mock();
    let rest_locals: Vec<M3> = (0..BONES.len())
        .map(|b| read9(&e, node_of(b) + LOCAL_ROTATION))
        .collect();
    clear();
    *WRITTEN.lock().unwrap_or_else(|p| p.into_inner()) = Vec::new();
    // A frame before the hit: the rig's last pose is the animation's.
    apply(&mut e);
    let mass = 80.0;
    hit(&mut e, ACTOR, v * mass, mass, point, dir, false);
    let hit_at = Instant::now();
    let mut samples = Vec::new();
    while hit_at.elapsed().as_secs_f32() < seconds {
        std::thread::sleep(std::time::Duration::from_millis(16));
        // The game's animation update: the spine's first bone turned (its chain follows).
        let t = hit_at.elapsed().as_secs_f32();
        let extra = anim(t).map_or(IDENTITY, ragdoll_core::flinch::rotation);
        let (up_r, _) = (rest(None), ());
        let local = mul(
            &mul(&transpose(&up_r), &mul(&extra, &up_r)),
            &rest_locals[1],
        );
        write9(&mut e, node_of(1) + LOCAL_ROTATION, &local);
        update_worlds(&mut e);
        let anim_spine_top = read9(&e, node_of(SPINE_TOP) + WORLD_ROTATION);
        let anim_checked = CHECKED.map(|b| read9(&e, node_of(b) + WORLD_ROTATION));
        apply(&mut e);
        let r = mul(
            &read9(&e, node_of(SPINE_TOP) + WORLD_ROTATION),
            &transpose(&anim_spine_top),
        );
        let rv = ragdoll_core::flinch::rotation_vector(&r);
        let drawn = CHECKED
            .iter()
            .zip(&anim_checked)
            .map(|(&b, a)| {
                let d = mul(&read9(&e, node_of(b) + WORLD_ROTATION), &transpose(a));
                length(ragdoll_core::flinch::rotation_vector(&d)).to_degrees()
            })
            .fold(0.0, f32::max);
        let (computed, delta, stretch) = RIGS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .find(|r| r.actor == ACTOR)
            .map_or((0.0, 0.0, 0.0), |r| {
                (r.shown_turn, r.drawn_delta, r.drawn_length)
            });
        samples.push(Sample {
            t: hit_at.elapsed().as_secs_f32(),
            turn: length(rv).to_degrees(),
            yaw: rv[2].to_degrees(),
            drawn,
            computed,
            delta,
            stretch,
        });
        after_draw(&mut e);
        unwrite(&mut e);
    }
    clear();
    if std::env::var_os("NV_MOCK_TRACE").is_some() {
        let t: Vec<String> = samples
            .iter()
            .map(|x| {
                format!(
                    "{:.3}:{:.1}/{:.1} d{:.2} l{:.2}",
                    x.t, x.turn, x.yaw, x.delta, x.stretch
                )
            })
            .collect();
        eprintln!("{v} m/s at {point:?}: {}", t.join(" "));
    }
    samples
}

fn peak(samples: &[Sample], from: f32, to: f32) -> f32 {
    samples
        .iter()
        .filter(|s| s.t >= from && s.t <= to)
        .map(|s| s.turn)
        .fold(0.0, f32::max)
}

/// The drawn flinch reaches the nodes the world pass draws: a hit through the chest's middle
/// from the front at 0.32 m/s (a 9mm's) turns the spine's top at least 5 degrees and at 0.79 m/s
/// (a .44's) at least 12 at 0.08 to 0.12 s, never more than 1.2 times its target (6, 14); it is
/// under 2 degrees by 0.6 s.
#[test]
fn the_drawn_flinch_reaches_the_spine_tops_node() {
    let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    for (v, least, target) in [(0.32f32, 5.0f32, 6.0f32), (0.79, 12.0, 14.0)] {
        let s = run(v, [0.0, -1.0, 0.0], [0.0, 9.0, 90.0], 0.75);
        let at_peak = peak(&s, 0.08, 0.14);
        let most = peak(&s, 0.0, 10.0);
        let late = s
            .iter()
            .filter(|x| x.t >= 0.6)
            .map(|x| x.turn)
            .fold(0.0, f32::max);
        let trace: Vec<String> = s
            .iter()
            .map(|x| format!("{:.3}:{:.1}", x.t, x.turn))
            .collect();
        assert!(
            at_peak >= least,
            "{v} m/s: the spine's top {at_peak:.1} degrees at 0.08-0.14 s, want {least}: {trace:?}"
        );
        assert!(
            most <= 1.2 * target,
            "{v} m/s: {most:.1} degrees at most, over 1.2 x {target}: {trace:?}"
        );
        assert!(late < 2.0, "{v} m/s: {late:.1} degrees by 0.6 s: {trace:?}");
    }
}

/// A hit on a clavicle twists the spine's top about the vertical: at least 6 degrees for a .44's
/// 0.79 m/s on the right clavicle from the front.
#[test]
fn a_clavicle_hit_twists_the_drawn_torso() {
    let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let s = run(0.79, [0.0, -1.0, 0.0], [8.0, 3.0, 100.0], 0.4);
    let twist = s
        .iter()
        .filter(|x| x.t <= 0.3)
        .map(|x| x.yaw.abs())
        .fold(0.0, f32::max);
    let trace: Vec<String> = s
        .iter()
        .map(|x| format!("{:.3}:{:.1}", x.t, x.yaw))
        .collect();
    assert!(twist >= 6.0, "twist {twist:.1} degrees: {trace:?}");
}

/// The game's own hit animation (a stagger) starting the frame after the hit, swinging the spine
/// back 18 degrees in two frames and easing back over half a second: the drawn flinch still
/// reaches the spine's top over that animation (at least 12 degrees for 0.79 m/s at 0.08-0.14 s).
#[test]
fn the_drawn_flinch_shows_over_the_games_stagger() {
    let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let stagger = |t: f32| {
        let a = 18f32.to_radians();
        let k = if t < 0.035 {
            t / 0.035
        } else {
            (1.0 - (t - 0.035) / 0.5).max(0.0)
        };
        Some([-a * k, 0.0, 0.0])
    };
    let s = run_with(0.79, [0.0, -1.0, 0.0], [0.0, 9.0, 90.0], 0.75, stagger);
    let at_peak = peak(&s, 0.08, 0.14);
    let trace: Vec<String> = s
        .iter()
        .map(|x| format!("{:.3}:{:.1}", x.t, x.turn))
        .collect();
    assert!(
        at_peak >= 12.0,
        "{at_peak:.1} degrees over the stagger: {trace:?}"
    );
}

/// No frame jumps (Patrick's a45deed clips: 12.97 degrees drawn one frame after a .44's hit,
/// then 0.50): from the animation before the hit (nothing off it), each frame's drawn turn moves
/// at most 1 degree beyond the computed pose's own move, and the computed pose moves no faster
/// than a quarter over a bone's turn off its pose (`active::SPIN_MOST`, 6 rad/s: the clavicle
/// hit's 20 degrees rise at about 300 degrees a second, the body's own share on top), the first
/// frame from nothing included; also
/// over the game's stagger starting under the hit.
#[test]
fn no_drawn_frame_jumps_from_its_neighbours() {
    let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let stagger = |t: f32| {
        let a = 18f32.to_radians();
        let k = if t < 0.035 {
            t / 0.035
        } else {
            (1.0 - (t - 0.035) / 0.5).max(0.0)
        };
        Some([-a * k, 0.0, 0.0])
    };
    let runs = [
        run(0.32, [0.0, -1.0, 0.0], [0.0, 9.0, 90.0], 0.6),
        run(0.79, [0.0, -1.0, 0.0], [0.0, 9.0, 90.0], 0.6),
        run(0.79, [0.0, -1.0, 0.0], [8.0, 3.0, 100.0], 0.6),
        run_with(0.79, [0.0, -1.0, 0.0], [0.0, 9.0, 90.0], 0.6, stagger),
    ];
    for s in &runs {
        let trace: Vec<String> = s
            .iter()
            .map(|x| format!("{:.3}:{:.1}/{:.1}", x.t, x.drawn, x.computed))
            .collect();
        let mut before = (0.0f32, 0.0f32);
        let mut t0 = 0.0f32;
        for x in s {
            let most = 1.25 * active::SPIN_MOST.to_degrees() * (x.t - t0) + 0.5;
            t0 = x.t;
            let beyond = ((x.drawn - before.0) - (x.computed - before.1)).abs();
            assert!(
                beyond <= 1.0,
                "{beyond:.2} degrees beyond at {:.3}: {trace:?}",
                x.t
            );
            assert!(
                (x.computed - before.1).abs() <= most,
                "the computed pose moved {:.1} degrees in a frame at {:.3}: {trace:?}",
                x.computed - before.1,
                x.t
            );
            before = (x.drawn, x.computed);
        }
    }
}

/// `TESObjectREFR::SetLocationOnReference`, the actor's character controller,
/// `PathingLocation(point)` and its position, `ResolveToClosestNavmeshAndTriangle`,
/// `IsValidLocation` (`crate::knockdown`'s).
const SET_LOCATION: u32 = 0x0057_5830;
const CHAR_CONTROLLER: u32 = 0x0093_06d0;
const PATHING_LOCATION_AT: u32 = 0x006d_ce10;
const PATHING_POSITION: u32 = 0x04;
const RESOLVE_TO_NAVMESH: u32 = 0x006d_dc00;
const IS_VALID_LOCATION: u32 = 0x006d_ed70;

/// The mock's world for catch steps: setting the actor's location moves its reference at once
/// and its 3D's root at the next frame (the game shows it then); the navmesh floor is at 0.
fn with_world(e: &mut Engine) {
    e.register_double(SET_LOCATION, |e, a| {
        let at = read3(e, a[1]);
        write3(e, a[0] + REFR_POSITION, at);
        0u32.into_ret()
    });
    e.register_double(CHAR_CONTROLLER, |_, _| 0u32.into_ret());
    e.register_double(PATHING_LOCATION_AT, |e, a| {
        let at = read3(e, a[1]);
        write3(e, a[0] + PATHING_POSITION, [at[0], at[1], 0.0]);
        0u32.into_ret()
    });
    e.register_double(RESOLVE_TO_NAVMESH, |_, _| 1u32.into_ret());
    e.register_double(IS_VALID_LOCATION, |_, _| 1u32.into_ret());
}

/// What a run with catch steps drew: the most `drawn_delta` and `drawn_length` of the rig (the
/// pose log's), the most any bone's drawn joint moved in a frame beyond the root's own move
/// (units), and how far the actor followed its feet.
struct Steps {
    delta: f32,
    length: f32,
    jump: f32,
    followed: f32,
    trace: Vec<String>,
}

/// A hit at `v` m/s from the front with its push (catch steps), then the frames for `seconds`.
fn run_steps(v: f32, seconds: f32) -> Steps {
    let mut e = mock();
    with_world(&mut e);
    clear();
    *WRITTEN.lock().unwrap_or_else(|p| p.into_inner()) = Vec::new();
    apply(&mut e);
    let mass = 80.0;
    let dir = [0.0, -1.0, 0.0];
    hit(&mut e, ACTOR, v * mass, mass, [0.0, 9.0, 90.0], dir, true);
    push(&mut e, ACTOR, v * mass, mass, dir, 0);
    let hit_at = Instant::now();
    let mut out = Steps {
        delta: 0.0,
        length: 0.0,
        jump: 0.0,
        followed: 0.0,
        trace: Vec::new(),
    };
    let mut before: Option<(Vec<V3>, V3)> = None;
    while hit_at.elapsed().as_secs_f32() < seconds {
        std::thread::sleep(std::time::Duration::from_millis(16));
        // The game's frame: the root at the reference (a move set last frame shows now).
        let reference = read3(&e, ACTOR + REFR_POSITION);
        write3(&mut e, ROOT + LOCAL_TRANSLATION, reference);
        update_worlds(&mut e);
        apply(&mut e);
        let drawn: Vec<V3> = (0..BONES.len())
            .map(|b| read3(&e, node_of(b) + WORLD_TRANSLATION))
            .collect();
        let root_now = read3(&e, ROOT + WORLD_TRANSLATION);
        if let Some((d0, r0)) = &before {
            let root_move = sub(root_now, *r0);
            let _ = root_move;
            // The drawn pelvis's move this frame (where the body stands: a slide moves it).
            out.jump = out.jump.max(length(sub(drawn[0], d0[0])));
        }
        before = Some((drawn, root_now));
        if let Some(r) = RIGS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .find(|r| r.actor == ACTOR)
        {
            out.delta = out.delta.max(r.drawn_delta);
            out.length = out.length.max(r.drawn_length);
            out.trace.push(format!(
                "{:.3}: delta {:.2} length {:.2} ref ({:.1}, {:.1}) steps {}",
                hit_at.elapsed().as_secs_f32(),
                r.drawn_delta,
                r.drawn_length,
                reference[0],
                reference[1],
                r.balance.is_some()
            ));
        }
        after_draw(&mut e);
        unwrite(&mut e);
    }
    out.followed = length(read3(&e, ACTOR + REFR_POSITION));
    clear();
    out
}

/// The actor follows its feet without a slide (Patrick's a45deed clip: 8 units in one frame, a
/// 15 pixel slide, `drawn_delta` 7.8, `drawn_length` 3.4 at the thigh): over catch steps the
/// pose log's `drawn_delta` stays under 1 unit and `drawn_length` under 0.5, and the pelvis is
/// never drawn moving more than 3 units in a frame (the push itself moves it about 1.3).
#[test]
fn catch_steps_follow_the_feet_without_a_slide() {
    let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let s = run_steps(1.1, 1.6);
    let trace = s.trace.join("\n");
    assert!(
        s.followed > 2.0,
        "no follow of the feet to check ({:.1} units):\n{trace}",
        s.followed
    );
    assert!(s.delta < 1.0, "drawn_delta {:.2}:\n{trace}", s.delta);
    assert!(s.length < 0.5, "drawn_length {:.2}:\n{trace}", s.length);
    assert!(
        s.jump <= 3.0,
        "drawn {:.2} units in a frame:\n{trace}",
        s.jump
    );
}

/// The get-up takes the bones from the ragdoll without a snap (Patrick's a45deed clips: at knock
/// state 6 the game's get-up animation took them in 2 to 4 frames, a seated body half up in 3,
/// the legs flung into a handstand): a body lying on its back, its ragdoll pose kept, then the
/// get-up animation's first frame standing (the farthest it could be). The drawn pose starts at
/// the ragdoll's, no bone is drawn moving faster than 400 units a second, and it ends on the
/// animation.
#[test]
fn the_get_up_blends_from_the_ragdoll_without_a_snap() {
    let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let mut e = mock();
    clear();
    *WRITTEN.lock().unwrap_or_else(|p| p.into_inner()) = Vec::new();
    let standing_non_accum = (
        read9(&e, NON_ACCUM + LOCAL_ROTATION),
        read3(&e, NON_ACCUM + LOCAL_TRANSLATION),
    );
    // The ragdoll: the whole body tipped onto its back, its pelvis 10 units over the floor.
    let back = ragdoll_core::flinch::rotation([std::f32::consts::FRAC_PI_2, 0.0, 0.0]);
    write9(
        &mut e,
        NON_ACCUM + LOCAL_ROTATION,
        &mul(&back, &standing_non_accum.0),
    );
    write3(&mut e, NON_ACCUM + LOCAL_TRANSLATION, [0.0, 0.0, 10.0]);
    update_worlds(&mut e);
    let ragdoll: Vec<V3> = (0..BONES.len())
        .map(|b| read3(&e, node_of(b) + WORLD_TRANSLATION))
        .collect();
    e.mem.set_u8(PROCESS_AT + KNOCK_STATE, 3);
    note_ragdoll_pose(&mut e, ACTOR);
    // Knock state 6: the get-up animation's first frame (standing) in the bones.
    write9(&mut e, NON_ACCUM + LOCAL_ROTATION, &standing_non_accum.0);
    write3(&mut e, NON_ACCUM + LOCAL_TRANSLATION, standing_non_accum.1);
    update_worlds(&mut e);
    let anim: Vec<V3> = (0..BONES.len())
        .map(|b| read3(&e, node_of(b) + WORLD_TRANSLATION))
        .collect();
    e.mem.set_u8(PROCESS_AT + KNOCK_STATE, 6);
    *LAST.lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
    begin_get_up_blend(&mut e, ACTOR);
    let start = Instant::now();
    let mut before: Option<(Vec<V3>, f32)> = None;
    let (mut fastest, mut first_off, mut last_off) = (0.0f32, None, f32::MAX);
    let mut trace = Vec::new();
    while start.elapsed().as_secs_f32() < 0.8 {
        std::thread::sleep(std::time::Duration::from_millis(16));
        update_worlds(&mut e);
        apply(&mut e);
        let t = start.elapsed().as_secs_f32();
        let drawn: Vec<V3> = (0..BONES.len())
            .map(|b| read3(&e, node_of(b) + WORLD_TRANSLATION))
            .collect();
        let off_ragdoll = drawn
            .iter()
            .zip(&ragdoll)
            .map(|(a, b)| length(sub(*a, *b)))
            .fold(0.0, f32::max);
        last_off = drawn
            .iter()
            .zip(&anim)
            .map(|(a, b)| length(sub(*a, *b)))
            .fold(0.0, f32::max);
        first_off.get_or_insert(off_ragdoll);
        if let Some((d0, t0)) = &before {
            let v = drawn
                .iter()
                .zip(d0)
                .map(|(a, b)| length(sub(*a, *b)))
                .fold(0.0, f32::max)
                / (t - t0).max(1e-3);
            fastest = fastest.max(v);
            trace.push(format!(
                "{t:.3}: {v:.0} u/s, {last_off:.1} off the animation"
            ));
        }
        before = Some((drawn, t));
        after_draw(&mut e);
        unwrite(&mut e);
    }
    clear();
    let trace = trace.join("\n");
    let first = first_off.unwrap_or(f32::MAX);
    assert!(
        first < 12.0,
        "the first frame {first:.1} units off the ragdoll's pose:\n{trace}"
    );
    assert!(
        fastest <= 400.0,
        "a bone drawn at {fastest:.0} units/s:\n{trace}"
    );
    assert!(
        last_off < 0.5,
        "{last_off:.1} units off the animation at the end:\n{trace}"
    );
}
