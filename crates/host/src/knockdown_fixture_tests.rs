//! The knockdown's get-up run on an actor's 3D built from a node tree (`crate::rig_fixture`):
//! the ragdoll's bodies on the tree's nodes (none on the pelvis, as in the game: Patrick's a7a2d15
//! clips), the body lying on the floor, then [`get_ups`] each frame as the game's own runs it,
//! through the knock states to the get-up animation (knock state 6) and its drawn blend
//! (`crate::active_ragdoll`'s write, read back from the nodes).

use super::*;
use crate::rig_fixture::{self, Fixture, Ground, ACTOR, ACTOR_ID};
use ragdoll_core::flinch::{length, rotation, sub, IDENTITY, M3};

/// One at a time with the reactions' own tests: the knockdowns' and the reactions' lists are the
/// process's.
use crate::active_ragdoll::tests::SERIAL;

/// A knock-down's get-up bookkeeping for the fixture's actor, as `knock_down` leaves it, the body
/// lying for `lying` seconds already.
fn knocked(stood: [f32; 3], lying: f32) {
    let since = Instant::now() - Duration::from_secs_f32(lying + FLIGHT.as_secs_f32());
    let mut ups = GET_UPS.lock().unwrap_or_else(|p| p.into_inner());
    // Handed down by New Physics (`hand_over` made its bookkeeping): kept so.
    let from_np = ups.iter().any(|g| g.actor == ACTOR && g.from_np);
    ups.clear();
    ups.push(GetUp {
        actor: ACTOR,
        id: ACTOR_ID,
        start: stood,
        since,
        lying_since: None,
        still_frames: 0,
        held_said: false,
        allowed: false,
        placed: None,
        next_log: 0.0,
        fall_dir: [0.0, -1.0, 0.0],
        braced: false,
        nudges: 0,
        nudge_at: since,
        blended: false,
        from_np,
        rolls: 0,
        roll_at: since,
    });
}

fn get_up_of() -> Option<GetUp> {
    GET_UPS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|g| g.actor == ACTOR)
        .copied()
}

/// One frame of a get-up as drawn: seconds, the knock state, the drawn joints of the rig's bones
/// (the pelvis, the spine's top, the head, the feet), the animated pelvis and head (none before the
/// animation arrives), the lowest drawn node of the whole skeleton (game units), the actor's
/// reference.
#[derive(Clone, Debug)]
struct Frame {
    t: f32,
    state: u8,
    drawn: Vec<[f32; 3]>,
    pelvis: [f32; 3],
    head: [f32; 3],
    anim_pelvis: Option<[f32; 3]>,
    anim_head: Option<[f32; 3]>,
    anim: Option<Vec<[f32; 3]>>,
    /// The same joints' drawn world rotations, and the animation's.
    drawn_r: Vec<M3>,
    anim_r: Option<Vec<M3>>,
    lowest: f32,
    reference: [f32; 3],
    /// The get-up blend drew this frame.
    blending: bool,
}

/// What a get-up did.
pub(crate) struct Run {
    set_at: Option<[f32; 3]>,
    pelvis_found: bool,
    blend_from: f32,
    frames: Vec<Frame>,
    /// The clips' sample each frame (`active_ragdoll::sample_fields`) with the frame's drawn
    /// pelvis z.
    samples: Vec<(u8, f32, String)>,
}

/// The get-up animation at `tau` seconds into it (knock state 6, the animation's own time): the
/// skeleton lying on its back, then pivoting up about its feet to standing over [`RISE`] seconds,
/// at the reference `at` (every node's world).
fn get_up_animation(f: &mut Fixture, at: [f32; 3], tau: f32) -> Vec<(M3, [f32; 3])> {
    let root = f.at[0];
    rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, at);
    f.animate();
    let standing = f.worlds();
    let foot = f.world("Bip01 L Foot").unwrap().1;
    let pivot = [at[0], foot[1], foot[2]];
    // Lying, its pelvis is at the reference (the game aligns its get-up to the body in knock
    // state 5): the pivot moved by what the lying pelvis lies off it.
    let pelvis = f.world("Bip01 Pelvis").unwrap().1;
    let lying = rotation([std::f32::consts::FRAC_PI_2, 0.0, 0.0]);
    let pelvis0 = ragdoll_core::flinch::add(
        pivot,
        ragdoll_core::flinch::apply_m(&lying, sub(pelvis, pivot)),
    );
    let shift = [at[0] - pelvis0[0], at[1] - pelvis0[1], 0.0];
    let pivot = ragdoll_core::flinch::add(pivot, shift);
    let standing: Vec<(M3, [f32; 3])> = standing
        .iter()
        .map(|(r, t)| (*r, ragdoll_core::flinch::add(*t, shift)))
        .collect();
    let k = ragdoll_core::active::ease((tau / RISE).clamp(0.0, 1.0));
    let turn = rotation([std::f32::consts::FRAC_PI_2 * (1.0 - k), 0.0, 0.0]);
    // The root (the actor's node) stays where the actor is: the animation moves the bones under it.
    let root_world = f.world_of(root);
    standing
        .iter()
        .enumerate()
        .map(|(i, (r, t))| {
            if i == 0 {
                return root_world;
            }
            (
                ragdoll_core::flinch::mul(&turn, r),
                ragdoll_core::flinch::add(
                    pivot,
                    ragdoll_core::flinch::apply_m(&turn, sub(*t, pivot)),
                ),
            )
        })
        .collect()
}

/// How long the fixture's get-up animation takes to stand (s), how many frames into knock state 6
/// it arrives (the game's: 1 to 3), how many frames knock state 1 runs on after the get-up is
/// allowed.
const RISE: f32 = 1.5;
const ARRIVES: u32 = 2;
const STATE_1_AFTER: u32 = 4;
/// How fast the game's motors drive a body onto its animated joint (s to get there).
const DRIVE: f32 = 0.08;

/// A get-up on the tree `nodes` as the game runs it (Patrick's bdf710a review):
/// - the ragdoll lying on its back on the collision floor at 0 (the navmesh 8 under it), turned
///   35 degrees off the way the get-up animation will lie, its bodies driving the nodes (the
///   locals worked out from them);
/// - `get_ups` each frame; knock state 1 runs on [`STATE_1_AFTER`] frames after the get-up is
///   allowed, then 3 and 5 a frame each, then 6;
/// - in 6 the bodies still drive the nodes until the animation arrives ([`ARRIVES`] frames late);
///   from then the animation's locals are in the nodes and each body is driven onto its node's
///   animated pose (its velocity set to get there in a frame, before `get_ups` may hold it, then
///   moved by it), the drawn nodes following the bodies; after [`RISE`] it is standing, knock state
///   0, the animation alone;
/// - the reactions' frame each frame: the real write, update, skin read and put-back.
fn game_get_up(nodes: Vec<rig_fixture::Node>) -> Run {
    let mut f = Fixture::new(
        nodes,
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    crate::active_ragdoll::clear();
    clear();
    let reference = [0.0f32, 0.0, 0.0];
    rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, reference);
    // The ragdoll: the animation's first frame turned 35 degrees about the pelvis and lifted 2.
    let lying = get_up_animation(&mut f, reference, 0.0);
    let pelvis_i = f
        .nodes
        .iter()
        .position(|n| n.name == "Bip01 Pelvis")
        .unwrap();
    let about = lying[pelvis_i].1;
    let yaw = rotation([0.0, 0.0, 35f32.to_radians()]);
    let ragdoll: Vec<(M3, [f32; 3])> = lying
        .iter()
        .enumerate()
        .map(|(i, (r, t))| {
            if i == 0 {
                return (*r, *t);
            }
            (
                ragdoll_core::flinch::mul(&yaw, r),
                ragdoll_core::flinch::add(
                    ragdoll_core::flinch::add(
                        about,
                        ragdoll_core::flinch::apply_m(&yaw, sub(*t, about)),
                    ),
                    [0.0, 0.0, 2.0],
                ),
            )
        })
        .collect();
    f.set_worlds(&ragdoll);
    get_up_from(&mut f, ragdoll)
}

/// The get-up as [`game_get_up`] runs it, from the ragdoll lying as `ragdoll` (every node's world)
/// on the fixture `f` (the collision floor at 0): knock state 1, the get-up's bookkeeping, then
/// the knock states to the get-up animation and standing.
pub(crate) fn get_up_from(f: &mut Fixture, mut ragdoll: Vec<(M3, [f32; 3])>) -> Run {
    let reference = f.reference();
    let pelvis_i = f
        .nodes
        .iter()
        .position(|n| n.name == "Bip01 Pelvis")
        .unwrap();
    f.set_worlds(&ragdoll);
    f.set_knock_state(1);
    knocked(reference, 1.0);
    let bodies_at = f.body_nodes();
    let mut run = Run {
        set_at: None,
        pelvis_found: false,
        blend_from: 0.0,
        frames: Vec::new(),
        samples: Vec::new(),
    };
    let start = Instant::now();
    let mut last = start;
    let mut state: u8 = 1;
    let mut after_allowed = 0u32;
    let mut in_6 = 0u32;
    let mut arrived: Option<Instant> = None;
    let mut done: Option<Instant> = None;
    let mut first_blend_frame = true;
    while start.elapsed() < Duration::from_secs(6) {
        std::thread::sleep(Duration::from_millis(16));
        let dt = last.elapsed().as_secs_f32();
        last = Instant::now();
        // The game's knock state.
        if state == 1 && run.set_at.is_some() {
            after_allowed += 1;
            if after_allowed > STATE_1_AFTER {
                state = 3;
            }
        } else if state == 3 {
            state = 5;
        } else if state == 5 {
            state = 6;
        } else if state == 6 {
            in_6 += 1;
            if in_6 > ARRIVES && arrived.is_none() {
                arrived = Some(Instant::now());
            }
            if arrived.is_some_and(|a| a.elapsed().as_secs_f32() >= RISE) {
                state = 0;
                done = Some(Instant::now());
            }
        }
        if done.is_some_and(|d| d.elapsed() > Duration::from_millis(400)) {
            break;
        }
        f.set_knock_state(state);
        let reference = f.reference();
        // The animation's pose now (none before it arrives).
        let anim = arrived.map(|a| {
            let tau = a.elapsed().as_secs_f32();
            get_up_animation(f, reference, tau)
        });
        // The game's physics: a driven body's motor pulls its velocity toward what reaches its
        // animated joint in [`DRIVE`] seconds (half the way a frame; its velocity, which nv-rs may
        // have held last frame, kept); then each body moves by its velocity, a driven one turning
        // onto its animated rotation. Lying, the bodies are still.
        for (k, &i) in bodies_at.iter().enumerate() {
            let v = match (&anim, state) {
                (Some(a), 6) => {
                    let need = scale_v(sub(a[i].1, ragdoll[i].1), 1.0 / DRIVE);
                    let was = f.body_velocity(k);
                    [0, 1, 2].map(|c| was[c] + (need[c] - was[c]) * 0.5)
                }
                _ => [0.0; 3],
            };
            f.set_body_velocity(k, v);
            ragdoll[i].1 = ragdoll_core::flinch::add(ragdoll[i].1, scale_v(v, dt));
            if let (Some(a), 6) = (&anim, state) {
                ragdoll[i].0 = a[i].0;
            }
        }
        // The nodes: in the ragdoll the bodies drive them (and their locals), the animation's
        // own rule for a node with no body; in 6 once the animation is in, the animation's locals
        // with the bodies' worlds drawn; in 0 the animation alone.
        match (&anim, state) {
            (Some(a), 0) => {
                f.set_worlds(a);
                ragdoll = a.clone();
            }
            (Some(a), 6) => {
                f.set_worlds(a);
                let mut drawn = a.clone();
                for &i in &bodies_at {
                    drawn[i] = ragdoll[i];
                }
                // A node with no body under a body's node follows it with its animated local.
                follow_bodies(f, &mut drawn, &bodies_at);
                f.set_worlds_only(&drawn);
            }
            _ => {
                let mut drawn = ragdoll.clone();
                follow_bodies(f, &mut drawn, &bodies_at);
                f.set_worlds(&drawn);
            }
        }
        // The game's depenetration nudging the actor in knock state 6 (a unit a frame along x).
        if state == 6 {
            let mut r = f.reference();
            r[0] += 1.0;
            rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, r);
        }
        // nv-rs's frame (the knockdowns' bookkeeping: its holds of the bodies' velocities, which
        // the next step moves them by).
        get_ups(&mut f.e);
        if run.set_at.is_none() {
            if let Some((_, at)) = get_up_of().and_then(|g| g.placed) {
                run.set_at = Some(at);
                let bones = bodies(&mut f.e, ACTOR);
                run.pelvis_found = pelvis_pos(&mut f.e, ACTOR, &bones).is_some();
            }
        }
        // The reactions' frame: the write (a get-up blend), the world pass's read, the put-back.
        crate::active_ragdoll::apply(&mut f.e);
        crate::active_ragdoll::read_drawn(&mut f.e);
        let all = f.worlds();
        let at = |n: &str| f.world(n).unwrap().1;
        const NAMED: [&str; 5] = [
            "Bip01 Pelvis",
            "Bip01 Spine2",
            "Bip01 Head",
            "Bip01 L Foot",
            "Bip01 R Foot",
        ];
        let named: Vec<[f32; 3]> = NAMED.iter().map(|n| at(n)).collect();
        let named_r: Vec<M3> = NAMED.iter().map(|n| f.world(n).unwrap().0).collect();
        let index = |n: &str| f.nodes.iter().position(|x| x.name == n).unwrap();
        if state == 6 && arrived.is_some() && first_blend_frame {
            if let Some(rig) = crate::active_ragdoll::get_up_blend_from(ACTOR) {
                first_blend_frame = false;
                run.blend_from = rig;
            }
        }
        let head_i = f.nodes.iter().position(|n| n.name == "Bip01 Head").unwrap();
        let skeleton: Vec<usize> = (1..f.nodes.len())
            .filter(|&i| {
                f.nodes[i].name.starts_with("Bip01 ") && f.nodes[i].name != "Bip01 NonAccum"
            })
            .collect();
        run.frames.push(Frame {
            t: start.elapsed().as_secs_f32(),
            state,
            pelvis: named[0],
            head: named[2],
            drawn: named,
            anim_pelvis: anim.as_ref().map(|a| a[pelvis_i].1),
            anim_head: anim.as_ref().map(|a| a[head_i].1),
            anim: anim
                .as_ref()
                .map(|a| NAMED.iter().map(|n| a[index(n)].1).collect()),
            drawn_r: named_r,
            anim_r: anim
                .as_ref()
                .map(|a| NAMED.iter().map(|n| a[index(n)].0).collect()),
            lowest: skeleton
                .iter()
                .map(|&i| all[i].1[2])
                .fold(f32::INFINITY, f32::min),
            reference: f.reference(),
            blending: crate::active_ragdoll::get_up_blending(ACTOR),
        });
        crate::active_ragdoll::after_draw(&mut f.e);
        let drawn_pelvis = run.frames.last().map_or(0.0, |x| x.pelvis[2]);
        run.samples.push((
            state,
            drawn_pelvis,
            crate::active_ragdoll::sample_fields(ACTOR),
        ));
        crate::active_ragdoll::unwrite(&mut f.e);
    }
    crate::active_ragdoll::clear();
    clear();
    run
}

fn scale_v(v: [f32; 3], k: f32) -> [f32; 3] {
    v.map(|x| x * k)
}

/// Each node with no body is drawn from its parent's drawn world and its own local (as the game's
/// update places it), parents first.
fn follow_bodies(f: &mut Fixture, drawn: &mut [(M3, [f32; 3])], bodies_at: &[usize]) {
    for i in 1..f.nodes.len() {
        if bodies_at.contains(&i) {
            continue;
        }
        let Some(p) = f.nodes[i].parent else { continue };
        let n = f.at[i];
        let local_r = rig_fixture::read9(&f.e, n + rig_fixture::LOCAL_ROTATION);
        let local_t = rig_fixture::read3(&f.e, n + rig_fixture::LOCAL_TRANSLATION);
        let (pr, pt) = drawn[p];
        drawn[i] = (
            ragdoll_core::flinch::mul(&pr, &local_r),
            ragdoll_core::flinch::add(pt, ragdoll_core::flinch::apply_m(&pr, local_t)),
        );
    }
}

/// The checks of Patrick's bdf710a review on a get-up's frames (every frame of knock states 1 to
/// 6 and the frames after): the blend starts at least 20 units off the animation and eased (its
/// first frame drawn as the ragdoll lay); the drawn pelvis never more than 10 units over the
/// animation's; the torso never upside down (the head more than 20 under the pelvis) unless the
/// animation has it; no node more than 3 under the floor; no frame moving a drawn joint more than
/// 2 units beyond the animation's own move (the blend's end and the animation's taking over
/// included); the actor not moved in knock state 6. Answers a summary for the report.
/// The lowest of a get-up's drawn pelvis, head and feet (game units): the game's own get-up
/// log's (`crate::down_log`, target -3 over the floor or more).
pub(crate) fn get_up_lowest_logged(run: &Run) -> f32 {
    run.frames
        .iter()
        .map(|f| {
            f.pelvis[2]
                .min(f.head[2])
                .min(f.drawn[3][2])
                .min(f.drawn[4][2])
        })
        .fold(f32::MAX, f32::min)
}

/// A get-up's frames upside down (the head more than 20 under the pelvis, the animation not) and
/// its lowest drawn node (game units): [`check_get_up`]'s.
pub(crate) fn get_up_figures(run: &Run) -> (u32, f32) {
    let (mut inverted, mut lowest) = (0u32, f32::MAX);
    for w in run.frames.windows(2) {
        let b = &w[1];
        let anim_inverted = b
            .anim_head
            .zip(b.anim_pelvis)
            .is_some_and(|(h, p)| h[2] < p[2] - 20.0);
        if b.head[2] < b.pelvis[2] - 20.0 && !anim_inverted {
            inverted += 1;
        }
        lowest = lowest.min(b.lowest);
        if std::env::var_os("NP_TRACE").is_some() {
            eprintln!(
                "GETUP {:.3} ks{} pelvis {:?} head {:?} lowest {:.1} ref {:?}",
                b.t, b.state, b.pelvis, b.head, b.lowest, b.reference
            );
        }
    }
    (inverted, lowest)
}

fn check_get_up(run: &Run) -> String {
    let set = run.set_at.expect("never let up");
    assert!(run.pelvis_found, "no pelvis found");
    assert!(
        set[2].abs() < 0.5,
        "set at z {:.1}, not on the collision floor at 0",
        set[2]
    );
    assert!(
        run.blend_from >= 20.0,
        "the blend starts {:.1} off the animation",
        run.blend_from
    );
    // The pose log has the drawn get-up every frame of knock states 1 to 6 (bdf710a's had none):
    // the pelvis it says as drawn, the torso's up-axis, the animation's pelvis once it is in.
    for (state, pelvis, sample) in &run.samples {
        if !(1..=6).contains(state) {
            continue;
        }
        let said = sample
            .split("\"pelvis_over_floor\":")
            .nth(1)
            .and_then(|x| x.split(',').next())
            .and_then(|x| x.parse::<f32>().ok())
            .unwrap_or_else(|| panic!("no drawn pelvis in knock state {state}'s sample: {sample}"));
        assert!(
            (said - pelvis).abs() < 0.1,
            "the sample says the pelvis at {said}, drawn at {pelvis}: {sample}"
        );
        assert!(
            sample.contains(&format!("\"down\":{{\"knock_state\":{state},")),
            "{sample}"
        );
        assert!(sample.contains("\"torso_up\":"), "{sample}");
    }
    assert!(
        run.samples
            .iter()
            .any(|(s, _, x)| *s == 6 && !x.contains("\"anim_pelvis_over_floor\":null")),
        "no animation pelvis logged in knock state 6"
    );
    let trace: Vec<String> = run
        .frames
        .iter()
        .map(|x| {
            format!(
                "{:.3} ks{}{}: pelvis z {:.1} (anim {}), head z {:.1}, lowest {:.1}",
                x.t,
                x.state,
                if x.blending { " blend" } else { "" },
                x.pelvis[2],
                x.anim_pelvis.map_or("-".into(), |a| format!("{:.1}", a[2])),
                x.head[2],
                x.lowest
            )
        })
        .collect();
    let trace = trace.join("\n");
    let (mut over_anim, mut inverted, mut lowest, mut pop, mut moved) =
        (0.0f32, 0, f32::MAX, 0.0f32, 0.0f32);
    let (mut blend_fastest, mut turn_pop) = (0.0f32, 0.0f32);
    let ref6 = run
        .frames
        .iter()
        .find(|x| x.state == 6)
        .map(|x| x.reference);
    for w in run.frames.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if let Some(ap) = b.anim_pelvis {
            over_anim = over_anim.max(b.pelvis[2] - ap[2]);
        }
        let anim_inverted = b
            .anim_head
            .zip(b.anim_pelvis)
            .is_some_and(|(h, p)| h[2] < p[2] - 20.0);
        if b.head[2] < b.pelvis[2] - 20.0 && !anim_inverted {
            inverted += 1;
        }
        lowest = lowest.min(b.lowest);
        let own: Vec<[f32; 3]> = match (&a.anim, &b.anim) {
            (Some(x), Some(y)) => x.iter().zip(y).map(|(p, q)| sub(*q, *p)).collect(),
            _ => vec![[0.0; 3]; b.drawn.len()],
        };
        let step = b
            .drawn
            .iter()
            .zip(&a.drawn)
            .zip(&own)
            .map(|((q, p), o)| length(sub(sub(*q, *p), *o)))
            .fold(0.0, f32::max);
        // The blend itself eases the body over (at most 400 units a second along a bone's arc);
        // every other frame, its end and the animation's taking over among them, no pop.
        if b.blending {
            blend_fastest = blend_fastest.max(step / (b.t - a.t).max(1e-3));
        } else {
            pop = pop.max(step);
            // Each joint's turn this frame beyond the animation's own (the drawn rotation's
            // change against the animated one's).
            for (k, (q, p)) in b.drawn_r.iter().zip(&a.drawn_r).enumerate() {
                let own = match (&a.anim_r, &b.anim_r) {
                    (Some(x), Some(y)) => {
                        ragdoll_core::flinch::mul(&y[k], &ragdoll_core::flinch::transpose(&x[k]))
                    }
                    _ => IDENTITY,
                };
                let moved = ragdoll_core::flinch::mul(q, &ragdoll_core::flinch::transpose(p));
                turn_pop = turn_pop.max(rig_fixture::degrees_between(&moved, &own));
            }
        }
        if b.state == 6 {
            if let Some(r) = ref6 {
                moved = moved.max(length(sub(b.reference, r)));
            }
        }
    }
    let summary = format!(
        "pelvis found {}, set at z {:.2}, blend from {:.1} units, drawn pelvis over the animation's at most {over_anim:.1}, inverted frames {inverted}, lowest node z {lowest:.1}, largest per-frame move beyond the animation outside the blend {pop:.2} units and {turn_pop:.2} degrees, fastest drawn joint in the blend {blend_fastest:.0} units/s, actor moved in ks6 {moved:.2}",
        run.pelvis_found, set[2], run.blend_from
    );
    assert!(
        over_anim <= 10.0,
        "drawn pelvis {over_anim:.1} over the animation's:\n{trace}"
    );
    assert_eq!(inverted, 0, "{inverted} frames upside down:\n{trace}");
    assert!(
        lowest >= -3.0,
        "a node {lowest:.1} under the floor:\n{trace}"
    );
    assert!(
        pop <= 2.0,
        "a drawn joint moved {pop:.2} units in a frame beyond the animation:\n{trace}"
    );
    assert!(
        turn_pop <= 1.0,
        "a drawn joint turned {turn_pop:.2} degrees in a frame beyond the animation:\n{trace}"
    );
    assert!(moved <= 0.5, "the actor moved {moved:.2} in knock state 6");
    assert!(
        blend_fastest <= 420.0,
        "a joint drawn at {blend_fastest:.0} units/s in the blend:\n{trace}"
    );
    summary
}

/// The tester's anti-materiel get-up on FNV's own skeleton, as the game runs knock state 6.
#[test]
fn the_get_up_on_fnvs_skeleton() {
    let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let run = game_get_up(rig_fixture::fnv_skeleton());
    let summary = check_get_up(&run);
    eprintln!("RESULT get-up (FNV skeleton): {summary}");
}

/// The launch on FNV's skeleton: a reacting body (a hit's active ragdoll) switched to the ragdoll
/// at 4.35 m/s along the hit (`give_launch`), then the game's own knock-down push adding 170 units
/// a second along it (`KnockExplosion`, a7a2d15 held 470 for a 304 units/s hit). The launch check
/// (`launches`, each frame) holds the bodies' speed along it within 10% of the hit's through its
/// window and says the root body's velocity along the ground 0.1 s after the switch, also within
/// 10%, though the body falls (400 units a second off a ledge, then gravity).
#[test]
fn the_launch_is_the_hits_speed_on_fnvs_skeleton() {
    let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let mut f = Fixture::new(
        rig_fixture::fnv_skeleton(),
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    crate::active_ragdoll::clear();
    clear();
    LAUNCH_SAID.lock().unwrap().clear();
    f.animate();
    crate::active_ragdoll::apply(&mut f.e);
    let chest = f.world("Bip01 Spine1").unwrap().1;
    let dir = [0.0f32, -1.0, 0.0];
    let hit_v = 4.35f32;
    crate::active_ragdoll::hit(&mut f.e, ACTOR, 0.6 * 80.0, 80.0, chest, dir, false);
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(16));
        f.animate();
        crate::active_ragdoll::apply(&mut f.e);
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
    }
    f.set_knock_state(2);
    let launch = ragdoll_core::knock::launch_direction(dir);
    let speed = ragdoll_core::knock::launch_speed(hit_v);
    give_launch(&mut f.e, ACTOR, launch, speed);
    // The game's own push on top, after ours.
    let extra = 170.0 / rig_fixture::HK;
    let bodies_now = bodies(&mut f.e, ACTOR);
    for &(_, b) in &bodies_now {
        let v = linear(&mut f.e, b).unwrap();
        set_velocity(
            &mut f.e,
            SET_LINEAR_VELOCITY,
            b,
            [0, 1, 2].map(|i| v[i] + launch[i] * extra),
        );
    }
    LAUNCHES.lock().unwrap().push(Launch {
        actor: ACTOR,
        id: ACTOR_ID,
        direction: launch,
        speed,
        frames: 0,
        tries: 0,
        at: Instant::now(),
        said: false,
        held_said: false,
    });
    // Knocked off a ledge: the bodies falling 400 units a second, gravity on them each frame (the
    // 0.1 s line read 1.91 times the hit from such a fall: Patrick's bdf710a review, 38.67 s).
    let fall = |f: &mut Fixture, by: f32| {
        for &(_, b) in &bodies(&mut f.e, ACTOR) {
            let v = linear(&mut f.e, b).unwrap();
            set_velocity(
                &mut f.e,
                SET_LINEAR_VELOCITY,
                b,
                [v[0], v[1], v[2] - by / rig_fixture::HK],
            );
        }
    };
    fall(&mut f, 400.0);
    let mut most = 0.0f32;
    let start = Instant::now();
    let mut last = Instant::now();
    while start.elapsed() < Duration::from_millis(350) {
        let dt = last.elapsed().as_secs_f32();
        last = Instant::now();
        fall(&mut f, 686.0 * dt);
        launches(&mut f.e);
        let along: Vec<f32> = f
            .body_velocities()
            .iter()
            .map(|(_, v)| v[0] * launch[0] + v[1] * launch[1] + v[2] * launch[2])
            .collect();
        let mean = along.iter().sum::<f32>() / along.len() as f32;
        if start.elapsed() > Duration::from_millis(20) {
            most = most.max(mean);
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    crate::active_ragdoll::clear();
    clear();
    let said = LAUNCH_SAID.lock().unwrap().clone();
    eprintln!("launch: hit {speed:.0} units/s, the most along it after the hold {most:.0}; the 0.1 s line {said:?}");
    assert!(
        most <= 1.1 * speed,
        "{most:.0} units/s along the launch for a {speed:.0} hit"
    );
    let (what, v, _) = *said.first().expect("the 0.1 s velocity line said");
    assert!(
        (v / speed - 1.0).abs() <= 0.1,
        "the {what} at {v:.0} units/s for a {speed:.0} hit"
    );
}

/// A lying pose for [`get_ups`]: the get-up animation's first frame (on its back) at the
/// reference, turned `side` degrees onto its side about its length (the pelvis to the head; the
/// arm it turns onto laid back flat on the floor), lifted so its lowest node lies 2 units over
/// the floor (every node's world).
#[allow(clippy::needless_range_loop)]
fn lying_turned(f: &mut Fixture, side: f32) -> Vec<(M3, [f32; 3])> {
    let lying = get_up_animation(f, [0.0; 3], 0.0);
    let index = |n: &str| f.nodes.iter().position(|x| x.name == n).unwrap();
    let (pelvis_i, head_i) = (index("Bip01 Pelvis"), index("Bip01 Head"));
    let about = lying[pelvis_i].1;
    let along = ragdoll_core::flinch::unit(sub(lying[head_i].1, about)).unwrap();
    let turn = |angle: f32| {
        ragdoll_core::active::m_from_q(ragdoll_core::active::qexp(scale_v(
            along,
            angle.to_radians(),
        )))
    };
    let r = turn(side);
    let mut pose: Vec<(M3, [f32; 3])> = lying
        .iter()
        .enumerate()
        .map(|(i, (m, t))| {
            if i == 0 {
                return (*m, *t);
            }
            (
                ragdoll_core::flinch::mul(&r, m),
                ragdoll_core::flinch::add(about, ragdoll_core::flinch::apply_m(&r, sub(*t, about))),
            )
        })
        .collect();
    if side.abs() > 1.0 {
        let arm = if pose[index("Bip01 L Hand")].1[2] < pose[index("Bip01 R Hand")].1[2] {
            index("Bip01 L UpperArm")
        } else {
            index("Bip01 R UpperArm")
        };
        let back = turn(-side);
        let shoulder = pose[arm].1;
        for i in 0..f.nodes.len() {
            let mut p = Some(i);
            while let Some(j) = p {
                if j == arm {
                    break;
                }
                p = f.nodes[j].parent;
            }
            if p == Some(arm) {
                pose[i] = (
                    ragdoll_core::flinch::mul(&back, &pose[i].0),
                    ragdoll_core::flinch::add(
                        shoulder,
                        ragdoll_core::flinch::apply_m(&back, sub(pose[i].1, shoulder)),
                    ),
                );
            }
        }
    }
    let low = pose
        .iter()
        .skip(1)
        .map(|x| x.1[2])
        .fold(f32::INFINITY, f32::min);
    for x in pose.iter_mut().skip(1) {
        x.1[2] += 2.0 - low;
    }
    pose
}

/// What [`get_ups`] did with a still body lying as `pose`, held `held` seconds already, handed
/// down by New Physics or not, over 60 frames: whether its get-up was allowed, its rolls, its
/// front as it began (`knock::front_up`) and the spin given its bodies.
fn settle_run(pose_side: f32, held: f32, from_np: bool) -> (bool, usize, f32, [f32; 3]) {
    let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let mut f = Fixture::new(
        rig_fixture::fnv_skeleton(),
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    crate::active_ragdoll::clear();
    clear();
    rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, [0.0; 3]);
    let pose = lying_turned(&mut f, pose_side);
    f.set_worlds(&pose);
    f.set_knock_state(1);
    let since = Instant::now() - Duration::from_secs_f32(held + FLIGHT.as_secs_f32());
    GET_UPS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(GetUp {
            actor: ACTOR,
            id: ACTOR_ID,
            start: [0.0; 3],
            since,
            lying_since: Some(Instant::now() - Duration::from_secs_f32(held)),
            still_frames: 0,
            held_said: false,
            allowed: false,
            placed: None,
            next_log: 0.0,
            fall_dir: [0.0, 0.0, -1.0],
            braced: false,
            nudges: ragdoll_core::knock::NUDGES,
            nudge_at: since,
            blended: false,
            from_np,
            rolls: 0,
            roll_at: since,
        });
    let w = |n: &str| pose[f.nodes.iter().position(|x| x.name == n).unwrap()].1;
    let front = ragdoll_core::knock::front_up(
        w("Bip01 Spine1"),
        w("Bip01 Head"),
        w("Bip01 L UpperArm"),
        w("Bip01 R UpperArm"),
    );
    crate::room_events::set_on(true);
    let _ = crate::room_events::take();
    let mut spin = [0.0f32; 3];
    for _ in 0..60 {
        f.set_worlds(&pose);
        get_ups(&mut f.e);
        let s = f.body_spin(0);
        if length(s) > length(spin) {
            spin = s;
        }
        // The game's physics: the bodies still (a roll given them shows in their spin).
        for k in 0..f.body_nodes().len() {
            f.set_body_velocity(k, [0.0; 3]);
        }
    }
    let notes = crate::room_events::take();
    crate::room_events::set_on(false);
    let allowed = get_up_of().is_some_and(|g| g.allowed);
    let rolls = notes.iter().filter(|n| n.contains("[np settle]")).count();
    crate::active_ragdoll::clear();
    clear();
    (allowed, rolls, front, spin)
}

/// Patrick's 18:39 and 19:14 clips: bodies New Physics handed to the game's ragdoll lying on their
/// side were let up after 8 s ("held too long, not lying"), and the game's get-up, picked face up
/// or down from a side, played flipped (the pelvis 99 to 230 units up, upside down). Here,
/// through the get-up's bookkeeping (`get_ups`), with the body held 9 s already:
/// - handed down by New Physics on its side (its front 0.17 up), it is not let up but rolled
///   onto its back (its bodies given a spin about its length, toward its front facing up);
/// - the same on its back: let up (still and lying);
/// - the old knock-downs (not New Physics'): let up as before (held too long), never rolled.
#[test]
fn np_handed_bodies_get_up_only_on_their_back_or_front() {
    let (allowed, rolls, front, spin) = settle_run(80.0, 9.0, true);
    eprintln!(
        "RESULT np handed on its side: front {front:.2} up, allowed {allowed}, {rolls} rolls, \
         spin {spin:?}"
    );
    assert!(front.abs() < ragdoll_core::knock::FLAT_ENOUGH, "{front:.2}");
    assert!(!allowed, "let up on its side");
    assert!(rolls >= 1, "never rolled");
    assert!(
        (length(spin) - ragdoll_core::knock::ROLL_SPEED).abs() < 0.1,
        "spin {spin:?}"
    );
    let (allowed, rolls, front, _) = settle_run(0.0, 9.0, true);
    eprintln!(
        "RESULT np handed on its back: front {front:.2} up, allowed {allowed}, {rolls} rolls"
    );
    assert!(front >= ragdoll_core::knock::FLAT_ENOUGH, "{front:.2}");
    assert!(allowed && rolls == 0);
    let (allowed, rolls, _, _) = settle_run(80.0, 9.0, false);
    eprintln!("RESULT old knock-down on its side: allowed {allowed}, {rolls} rolls");
    assert!(allowed && rolls == 0);
}
