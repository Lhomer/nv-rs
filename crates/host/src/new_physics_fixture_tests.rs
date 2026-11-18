//! New Physics on FNV's own skeleton (`crate::rig_fixture::fnv_skeleton`, the real node tree laid
//! out in the engine's memory): the claim, the hit, and each frame the real write (the locals
//! solved from the simulated bodies), the engine's update, the read-back and the put-back, as the
//! game's frame runs them (`crate::active_ragdoll::apply`, `after_draw`, `unwrite`). The animation
//! is the skeleton's own pose; the floor the collision floor at 0.

use super::*;
use crate::rig_fixture::{self, Fixture, Ground, ACTOR};
use std::time::Duration;

/// What a run drew.
struct Run {
    /// The most the drawn (read back from the nodes after the update) was off the simulated
    /// bodies in any frame (degrees, units), and the most a node was off its animation after the
    /// put-back.
    readback: (f32, f32),
    put_back: f32,
    /// The spine's top's peak off its animation (degrees), the lowest node (units over the
    /// floor), the largest frame-to-frame move of a node after the hit (units, degrees).
    spine_peak: f32,
    lowest: f32,
    step_most: (f32, f32),
    /// The actor's reference's move, the catching steps, whether the current reactions had it.
    moved: V3,
    steps: u32,
    old_reactions: bool,
    claimed: bool,
    /// The clips' `"np"` samples: how many, the most spine and capture point outside they gave,
    /// the most catching steps, whether a step's swing showed, the sample once let go.
    samples: u32,
    sample_spine: f32,
    sample_outside: f32,
    sample_steps: u32,
    sample_swinging: bool,
    sample_after: String,
}

/// A number field of a sample (`"name":value`).
fn field(sample: &str, name: &str) -> Option<f32> {
    let at = sample.find(&format!("\"{name}\":"))? + name.len() + 3;
    let end = sample[at..].find([',', '}', ']'])? + at;
    sample[at..end].parse().ok()
}

fn np_run(speed: f32, seconds: f32, on: bool) -> Run {
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut f = Fixture::new(
        rig_fixture::fnv_skeleton(),
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    f.make_character();
    crate::active_ragdoll::clear();
    let was = crate::gunplay::tuning();
    crate::gunplay::set_tuning(crate::gunplay::Tuning {
        new_physics: on,
        ..was
    });
    let root = f.at[0];
    let body_nodes: Vec<usize> = model()
        .bodies
        .iter()
        .map(|b| f.nodes.iter().position(|n| n.name == b.node).unwrap())
        .collect();
    let spine = model().body("Bip01 Spine2").unwrap();
    f.animate();
    let claimed = claim(&mut f.e, ACTOR);
    let mut run = Run {
        readback: (0.0, 0.0),
        put_back: 0.0,
        spine_peak: 0.0,
        lowest: f32::INFINITY,
        step_most: (0.0, 0.0),
        moved: [0.0; 3],
        steps: 0,
        old_reactions: false,
        claimed,
        samples: 0,
        sample_spine: 0.0,
        sample_outside: 0.0,
        sample_steps: 0,
        sample_swinging: false,
        sample_after: String::new(),
    };
    let start = Instant::now();
    let mut hit_done = false;
    let mut before: Option<Vec<(M3, V3)>> = None;
    while start.elapsed() < Duration::from_secs_f32(seconds) {
        std::thread::sleep(Duration::from_millis(16));
        // The game's frame: the root where the actor is, the animation's pose.
        let reference = f.reference();
        rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, reference);
        f.animate();
        let anim: Vec<(M3, V3)> = body_nodes.iter().map(|&i| f.world_of(f.at[i])).collect();
        if !hit_done && start.elapsed() > Duration::from_millis(200) {
            hit_done = true;
            let chest = anim[spine].1;
            hit(
                &mut f.e,
                ACTOR,
                speed * model().mass,
                add(chest, [0.0, 8.0, 4.0]),
                [0.0, -1.0, 0.0],
            );
        }
        crate::active_ragdoll::apply(&mut f.e);
        crate::active_ragdoll::read_drawn(&mut f.e);
        let now: Vec<(M3, V3)> = body_nodes.iter().map(|&i| f.world_of(f.at[i])).collect();
        if let Some((drawn, _)) = drawn(ACTOR) {
            for (n, d) in now.iter().zip(&drawn) {
                run.readback.0 = run
                    .readback
                    .0
                    .max(ragdoll_core::np_skeleton::angle_between(&n.0, &d.0).to_degrees());
                run.readback.1 = run.readback.1.max(length(sub(n.1, d.1)));
            }
        }
        run.spine_peak = run.spine_peak.max(
            ragdoll_core::np_skeleton::angle_between(&now[spine].0, &anim[spine].0).to_degrees(),
        );
        let lowest = (1..f.nodes.len())
            .filter(|&i| f.nodes[i].name.starts_with("Bip01 "))
            .map(|i| f.world_of(f.at[i]).1[2])
            .fold(f32::INFINITY, f32::min);
        run.lowest = run.lowest.min(lowest);
        if let (Some(b), true) = (&before, hit_done) {
            for (x, y) in b.iter().zip(&now) {
                // Less the animation's own move (the actor moved by its steps).
                run.step_most.0 = run.step_most.0.max(length(sub(y.1, x.1)));
                run.step_most.1 = run
                    .step_most
                    .1
                    .max(ragdoll_core::np_skeleton::angle_between(&y.0, &x.0).to_degrees());
            }
        }
        before = Some(now);
        if let Some((_, steps, moved)) = state(ACTOR) {
            run.steps = steps;
            run.moved = moved;
        }
        run.old_reactions |= crate::active_ragdoll::reacting(ACTOR) && !owns(ACTOR);
        // The clips' sample, as `crate::room_record` takes it each frame.
        let sample = crate::active_ragdoll::sample_fields(ACTOR);
        let np = &sample[sample.find("\"np\":").expect("an np field")..];
        if np.starts_with("\"np\":{") {
            run.samples += 1;
            run.sample_spine = run.sample_spine.max(field(np, "spine").unwrap_or(f32::NAN));
            run.sample_outside = run
                .sample_outside
                .max(field(np, "outside").unwrap_or(f32::NAN));
            run.sample_steps = run
                .sample_steps
                .max(field(np, "steps").unwrap_or(0.0) as u32);
            run.sample_swinging |= field(np, "swinging").is_some();
        } else {
            run.sample_after = np.to_string();
        }
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
        // Put back: the nodes as the animation had them.
        for (k, &i) in body_nodes.iter().enumerate() {
            let w = f.world_of(f.at[i]);
            run.put_back = run.put_back.max(length(sub(w.1, anim[k].1)));
        }
    }
    crate::active_ragdoll::clear();
    crate::gunplay::set_tuning(was);
    run
}

/// A 0.8 m/s chest hit on FNV's skeleton with New Physics on, through the real write, update,
/// read-back and put-back: drawn is simulated within 0.5 degrees and 0.5 units every frame; the
/// game's own locals are back after the draw; the spine's top peaks 11 to 17 degrees (the lab's
/// 13); it takes a catching step and the actor's reference moves back with it; nothing more than
/// 3 units under the floor; no node jumps (at most 8 units or 25 degrees a frame); the current
/// reactions never touch it.
#[test]
fn np_draws_what_it_simulates_on_fnvs_skeleton() {
    let r = np_run(0.8, 3.0, true);
    eprintln!(
        "RESULT np on FNV's skeleton 0.8 m/s: read-back {:.3} deg / {:.3} u, put back {:.3} u, spine peak {:.1} deg, {} catching steps, moved ({:.1}, {:.1}), lowest {:.1} u, step most {:.1} u / {:.1} deg; {} np samples, spine {:.1} deg, capture point {:.2} m out, {} steps, a swing {}",
        r.readback.0, r.readback.1, r.put_back, r.spine_peak, r.steps, r.moved[0], r.moved[1], r.lowest, r.step_most.0, r.step_most.1, r.samples, r.sample_spine, r.sample_outside, r.sample_steps, r.sample_swinging
    );
    assert!(r.claimed, "not taken");
    assert!(
        r.readback.0 <= 0.5 && r.readback.1 <= 0.5,
        "drawn off simulated {:?}",
        r.readback
    );
    assert!(
        r.put_back < 0.01,
        "the animation not put back: {:.3} units",
        r.put_back
    );
    assert!(
        (11.0..=17.0).contains(&r.spine_peak),
        "spine top {:.1} degrees",
        r.spine_peak
    );
    assert!(r.steps >= 1, "no catching step");
    assert!(
        r.moved[1] < -5.0,
        "the actor did not move back with its step: {:?}",
        r.moved
    );
    assert!(r.lowest > -3.0, "a node {:.1} under the floor", r.lowest);
    assert!(
        r.step_most.0 < 8.0 && r.step_most.1 < 25.0,
        "{:?}",
        r.step_most
    );
    assert!(!r.old_reactions, "the current reactions had it");
    // The clips' sample says the same, every frame New Physics has it.
    assert!(r.samples > 30, "{} samples", r.samples);
    assert!(
        (r.sample_spine - r.spine_peak).abs() <= 0.5,
        "sample spine {:.1} for {:.1}",
        r.sample_spine,
        r.spine_peak
    );
    assert!(r.sample_outside > 0.0 && r.sample_steps == r.steps && r.sample_swinging);
}

/// A 0.3 m/s hit: no step, the actor not moved, the spine's top 5 to 8 degrees; drawn is
/// simulated.
#[test]
fn np_a_small_hit_takes_no_step_on_fnvs_skeleton() {
    let r = np_run(0.3, 1.5, true);
    eprintln!(
        "RESULT np on FNV's skeleton 0.3 m/s: read-back {:.3} deg / {:.3} u, spine peak {:.1} deg, {} steps, moved {:?}",
        r.readback.0, r.readback.1, r.spine_peak, r.steps, r.moved
    );
    assert!(
        r.readback.0 <= 0.5 && r.readback.1 <= 0.5,
        "{:?}",
        r.readback
    );
    assert_eq!(r.steps, 0);
    assert!(length(r.moved) < 0.5, "{:?}", r.moved);
    assert!(
        (5.0..=8.0).contains(&r.spine_peak),
        "spine top {:.1}",
        r.spine_peak
    );
}

/// The switch off (the default): nothing is taken; the current reactions stay.
#[test]
fn np_off_takes_nothing() {
    let r = np_run(0.8, 0.3, false);
    assert!(!r.claimed);
    assert_eq!(r.samples, 0);
    assert_eq!(r.sample_after, "\"np\":null");
    assert_eq!(r.steps, 0);
    assert!(!owns(ACTOR));
}

/// What a fall's hand-over did.
struct Fell {
    /// The knock state after it, the strength `KnockExplosion` was asked for, the bodies' velocity
    /// set (game units a second, the root body's) and the body's centre's at the hand-over (from
    /// [`falls`]), the most a body was off the drawn pose once moved (game units), the fastest a
    /// body rose (game units a second), whether the get-up watch has it.
    knock_state: u8,
    strength: f32,
    root_speed: f32,
    centre_speed: f32,
    body_off: f32,
    jump: f32,
    frame: f32,
    rising: f32,
    centre_rising: f32,
    havok_root: f32,
    havok_centre: f32,
    get_up: bool,
    steps: u32,
    frames_to_fall: u32,
}

fn fall_run(speed: f32) -> Fell {
    use std::sync::atomic::{AtomicU32, Ordering};
    static STRENGTH: AtomicU32 = AtomicU32::new(u32::MAX);
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut f = Fixture::new(
        rig_fixture::fnv_skeleton(),
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    f.make_character();
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    let was = crate::gunplay::tuning();
    crate::gunplay::set_tuning(crate::gunplay::Tuning {
        new_physics: true,
        ..was
    });
    // The game's calls of the hand-over: not an AI thread, `CanKnockDown` yes, `KnockExplosion`
    // into the ragdoll (knock state 2, its strength kept), a body's position and rotation set.
    f.e.register_double(0x008c_7aa0, |_, _| 0u32.into_ret());
    f.e.register_double(0x0088_45a0, |_, _| 1u32.into_ret());
    STRENGTH.store(u32::MAX, Ordering::Relaxed);
    f.e.register_double(crate::hit_force::KNOCK_EXPLOSION, |e, a| {
        STRENGTH.store(a[5], Ordering::Relaxed);
        e.mem.set_u8(a[0] + 0x13c, 2);
        0u32.into_ret()
    });
    f.e.register_double(0x00ca_8960, |e, a| {
        let p = rig_fixture::read3(e, a[1]);
        rig_fixture::write3(e, a[0] + 0x120, p);
        for i in 0..4 {
            let x = e.mem.f32(a[2] + 4 * i);
            e.mem.set_f32(a[0] + 0x160 + 4 * i, x);
        }
        0u32.into_ret()
    });
    let body_nodes: Vec<usize> = model()
        .bodies
        .iter()
        .map(|b| f.nodes.iter().position(|n| n.name == b.node).unwrap())
        .collect();
    let spine = model().body("Bip01 Spine2").unwrap();
    let root = f.at[0];
    f.animate();
    // Each Havok body kept on its node's animated pose, as the game's keyframing keeps it (Havok
    // units, its rotation as x, y, z, w).
    let bodies_of = |f: &mut Fixture| crate::knockdown::tests_bodies(&mut f.e, ACTOR);
    let keyframe = |f: &mut Fixture| {
        for (node, body) in bodies_of(f) {
            let (r, t) = f.world_of(node);
            rig_fixture::write3(&mut f.e, body + 0x120, scale(t, 1.0 / rig_fixture::HK));
            let q = ragdoll_core::active::qfrom_m(&r);
            for (i, x) in [q[1], q[2], q[3], q[0]].into_iter().enumerate() {
                f.e.mem.set_f32(body + 0x160 + 4 * i as u32, x);
            }
        }
    };
    keyframe(&mut f);
    assert!(claim(&mut f.e, ACTOR));
    let start = Instant::now();
    let mut hit_done = false;
    let mut frames_to_fall = 0;
    let mut steps = 0;
    let mut drawn_last: Vec<(M3, V3)> = Vec::new();
    // The last frame's time (s), as New Physics measures it: from one frame's run to the next.
    let mut frame = 0.0f32;
    let mut last_run: Option<Instant> = None;
    while start.elapsed() < Duration::from_secs_f32(2.0) {
        std::thread::sleep(Duration::from_millis(16));
        if f.e.mem.u8(rig_fixture::PROCESS_AT + 0x13c) != 0 {
            break;
        }
        let reference = f.reference();
        rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, reference);
        f.animate();
        keyframe(&mut f);
        if !hit_done && start.elapsed() > Duration::from_millis(200) {
            hit_done = true;
            let chest = f.world_of(f.at[body_nodes[spine]]).1;
            hit(
                &mut f.e,
                ACTOR,
                speed * model().mass,
                add(chest, [0.0, 8.0, 4.0]),
                [0.0, -1.0, 0.0],
            );
        }
        if let Some(t) = last_run {
            frame = t.elapsed().as_secs_f32();
        }
        last_run = Some(Instant::now());
        crate::active_ragdoll::apply(&mut f.e);
        // What the frame drew while New Physics had it: its body nodes' worlds as written.
        if owns(ACTOR) {
            drawn_last = body_nodes.iter().map(|&i| f.world_of(f.at[i])).collect();
        }
        if let Some((_, s, _)) = state(ACTOR) {
            steps = s;
        }
        if hit_done {
            frames_to_fall += 1;
        }
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
    }
    let falls = falls();
    let (root_speed, centre_speed) = falls.first().map_or((0.0, 0.0), |x| (x.root, x.centre));
    let handed = falls.first().map_or(Vec::new(), |x| x.pose.clone());
    let mut body_off = if handed.is_empty() { f32::MAX } else { 0.0f32 };
    let mut jump = 0.0f32;
    let mut rising = 0.0f32;
    let (mut lift, mut mass) = (0.0f32, 0.0f32);
    let mut momentum = [0.0f32; 3];
    let mut havok_root = 0.0f32;
    for (node, body) in bodies_of(&mut f) {
        let Some(k) = body_nodes.iter().position(|&i| f.at[i] == node) else {
            continue;
        };
        if let Some(d) = drawn_last.get(k) {
            let p = scale(rig_fixture::read3(&f.e, body + 0x120), rig_fixture::HK);
            // The body moved on from the pose last drawn through its frame at its speed: what is
            // left past that is a jump.
            let v = scale(rig_fixture::read3(&f.e, body + 0x10), rig_fixture::HK);
            jump = jump.max(length(sub(p, d.1)) - length(v) * frame);
        }
        if let Some(h) = handed.iter().find(|h| h.0 == node) {
            let p = scale(rig_fixture::read3(&f.e, body + 0x120), rig_fixture::HK);
            body_off = body_off.max(length(sub(p, h.1)));
        }
        let v = scale(rig_fixture::read3(&f.e, body + 0x10), rig_fixture::HK);
        momentum = add(momentum, scale(v, model().bodies[k].mass));
        if k == model().root {
            havok_root = length(v);
        }
        let up = v[2];
        rising = rising.max(up);
        lift += up * model().bodies[k].mass;
        mass += model().bodies[k].mass;
    }
    let out = Fell {
        knock_state: f.e.mem.u8(rig_fixture::PROCESS_AT + 0x13c),
        strength: f32::from_bits(STRENGTH.load(Ordering::Relaxed)),
        root_speed,
        centre_speed,
        body_off,
        jump,
        frame,
        rising,
        centre_rising: lift / mass.max(1.0),
        havok_root,
        havok_centre: length(momentum) / mass.max(1.0),
        get_up: crate::knockdown::in_get_up(ACTOR),
        steps,
        frames_to_fall,
    };
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    crate::gunplay::set_tuning(was);
    out
}

/// An anti-materiel hit (4.35 m/s on the body) is past any step: New Physics falls at once (no
/// step), hands the body to the game's ragdoll with no push (`KnockExplosion` at strength 0,
/// knock state 2), the bodies on New Physics' pose (within 1.5 units of the pose handed; at most 8
/// units past their own motion through a game frame from the pose last drawn) and moving as its
/// bodies were:
/// the body's centre within 10% of the hit's speed, the root body (the pelvis, below a chest hit)
/// at least 80% of it, the game's bodies moving within 2% of New Physics'; nothing flung up (the
/// body's centre rising no faster than 30 units a second, no limb faster than 300, a hand's
/// swing); the get-up watch has it.
#[test]
fn np_falls_into_the_games_ragdoll_on_fnvs_skeleton() {
    let r = fall_run(4.35);
    let hit = 4.35 * ragdoll_core::np_body::UNITS;
    eprintln!(
        "RESULT np fall 4.35 m/s: knock state {}, strength {}, root body {:.0} u/s and centre {:.0} u/s for a {hit:.0} u/s hit (the game's bodies {:.0} and {:.0}), bodies {:.2} u off the pose handed and {:.1} u past their motion from the last drawn, rising at most {:.0} u/s (centre {:.0}), steps {}, {} frames to the fall, get-up watch {}",
        r.knock_state, r.strength, r.root_speed, r.centre_speed, r.havok_root, r.havok_centre, r.body_off, r.jump, r.rising, r.centre_rising, r.steps, r.frames_to_fall, r.get_up
    );
    assert_eq!(r.knock_state, 2, "not in its ragdoll");
    assert_eq!(r.strength, 0.0, "KnockExplosion pushed");
    assert_eq!(r.steps, 0, "it stepped first");
    assert!(
        r.frames_to_fall <= 3,
        "{} frames to the fall",
        r.frames_to_fall
    );
    assert!(
        (r.centre_speed / hit - 1.0).abs() <= 0.1,
        "centre {:.0} for {hit:.0}",
        r.centre_speed
    );
    // The pelvis below a chest hit moves a little slower than the centre: the body turns too.
    assert!(
        r.root_speed >= 0.8 * hit,
        "root body {:.0} for {hit:.0}",
        r.root_speed
    );
    // The game's bodies move on as New Physics' did.
    assert!(
        (r.havok_root / r.root_speed - 1.0).abs() <= 0.02,
        "the game's root body {:.0}",
        r.havok_root
    );
    assert!(
        (r.havok_centre / r.centre_speed - 1.0).abs() <= 0.02,
        "the game's centre {:.0}",
        r.havok_centre
    );
    assert!(
        r.body_off < 1.5,
        "bodies {:.2} off the pose handed",
        r.body_off
    );
    // The arms slow behind the chest within the frame: what is left grows with the frame's time
    // squared (8 units up to a 23 ms frame; the debug build's frames run long).
    assert!(
        r.jump < 8.0f32.max(15000.0 * r.frame * r.frame),
        "bodies jumped {:.1} in a {:.0} ms frame",
        r.jump,
        r.frame * 1000.0
    );
    assert!(
        r.centre_rising < 30.0,
        "the centre rising at {:.0}",
        r.centre_rising
    );
    assert!(r.rising < 300.0, "a body rising at {:.0}", r.rising);
    assert!(r.get_up, "the get-up watch does not have it");
}
