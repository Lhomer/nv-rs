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
    let mut wall = Instant::now();
    while start.elapsed() < Duration::from_secs_f32(seconds) {
        std::thread::sleep(Duration::from_millis(16));
        // The game's frame seconds (New Physics' step): this run's wall clock frame.
        f.e.mem
            .set_f32(0x011d_ea30, wall.elapsed().as_secs_f32().min(0.05));
        wall = Instant::now();
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
    // M4's hand-over: the fall not kept downed (M6 keeps it).
    set_keep_down(false);
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
    let mut wall = Instant::now();
    while start.elapsed() < Duration::from_secs_f32(2.0) {
        std::thread::sleep(Duration::from_millis(16));
        // The game's frame seconds (New Physics' step): this run's wall clock frame.
        f.e.mem
            .set_f32(0x011d_ea30, wall.elapsed().as_secs_f32().min(0.05));
        wall = Instant::now();
        // The main thread's update: what waits for it (a fall's or a death's hand-over).
        main_thread(&mut f.e);
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
    set_keep_down(true);
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

/// What a downed run did (M6).
struct Downed {
    knock_state: u8,
    /// The labels logged (in order, each once), whether the actor was downed and stood again.
    labels: Vec<&'static str>,
    was_down: bool,
    stood_again: Option<f32>,
    /// Drawn against simulated at most (degrees, units) while downed; the most the actor's
    /// reference lay from the pelvis (flat, units) after it settled; whether the shots were
    /// withheld while it could not aim (no target), and never once it stood.
    readback: (f32, f32),
    ref_off: f32,
    withheld: bool,
    withheld_after: bool,
    /// The lowest bone under the floor (units); a death's bodies (placed, given velocities) and
    /// the most a placed body lay from the pose drawn (units).
    lowest: f32,
    death: Option<(usize, usize)>,
    death_off: f32,
}

/// A 4.35 m/s chest hit on FNV's skeleton with New Physics on and the fall kept (M6): the run
/// through the real write, update, read-back and put-back for `seconds`; dead at `die_at` (s
/// after the start) when given.
fn down_run(seconds: f32, die_at: Option<f32>) -> Downed {
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
    set_keep_down(true);
    let was = crate::gunplay::tuning();
    crate::gunplay::set_tuning(crate::gunplay::Tuning {
        new_physics: true,
        ..was
    });
    // The give-up's hand-over: not an AI thread, `CanKnockDown` yes, `KnockExplosion` into the
    // ragdoll (knock state 2).
    f.e.register_double(0x008c_7aa0, |_, _| 0u32.into_ret());
    f.e.register_double(0x0088_45a0, |_, _| 1u32.into_ret());
    f.e.register_double(crate::hit_force::KNOCK_EXPLOSION, |e, a| {
        e.mem.set_u8(a[0] + 0x13c, 2);
        0u32.into_ret()
    });
    // A body's position and rotation set (the death's hand-over).
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
    let pelvis_node = f.at[body_nodes[model().root]];
    f.animate();
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
    let mut run = Downed {
        knock_state: 0,
        labels: Vec::new(),
        was_down: false,
        stood_again: None,
        readback: (0.0, 0.0),
        ref_off: 0.0,
        withheld: false,
        withheld_after: false,
        lowest: f32::INFINITY,
        death: None,
        death_off: 0.0,
    };
    let start = Instant::now();
    let mut hit_done = false;
    let mut died = false;
    let mut drawn_at_death: Vec<(M3, V3)> = Vec::new();
    let mut wall = Instant::now();
    while start.elapsed() < Duration::from_secs_f32(seconds) {
        std::thread::sleep(Duration::from_millis(16));
        // The game's frame seconds (New Physics' step): this run's wall clock frame.
        f.e.mem
            .set_f32(0x011d_ea30, wall.elapsed().as_secs_f32().min(0.05));
        wall = Instant::now();
        // The pose last drawn, then the main thread's update: what waits for it (a fall's or a
        // death's hand-over takes that pose).
        if died && owns(ACTOR) {
            if let Some((p, _)) = drawn(ACTOR) {
                drawn_at_death = p;
            }
        }
        main_thread(&mut f.e);
        let t = start.elapsed().as_secs_f32();
        let reference = f.reference();
        rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, reference);
        f.animate();
        // The game keyframes a living actor's bodies to its animation, not a dead one's.
        if !died {
            keyframe(&mut f);
        }
        if !hit_done && t > 0.2 {
            hit_done = true;
            let chest = f.world_of(f.at[body_nodes[spine]]).1;
            hit(
                &mut f.e,
                ACTOR,
                4.35 * model().mass,
                add(chest, [0.0, 8.0, 4.0]),
                [0.0, -1.0, 0.0],
            );
        }
        if let Some(d) = die_at {
            if t > d && !died && owns(ACTOR) {
                died = true;
                if let Some((p, _)) = drawn(ACTOR) {
                    drawn_at_death = p;
                }
                f.e.mem.set_u32(ACTOR + 0x108, 1);
            }
        }
        crate::active_ragdoll::apply(&mut f.e);
        let down = is_down(ACTOR);
        if let Some((p, _)) = drawn(ACTOR) {
            if down {
                for (k, &i) in body_nodes.iter().enumerate() {
                    let w = f.world_of(f.at[i]);
                    run.readback.0 = run
                        .readback
                        .0
                        .max(ragdoll_core::np_skeleton::angle_between(&w.0, &p[k].0).to_degrees());
                    run.readback.1 = run.readback.1.max(length(sub(w.1, p[k].1)));
                }
            }
        }
        if let Some((_, _, _)) = state(ACTOR) {
            if let Some(l) = label_of(ACTOR) {
                if run.labels.last() != Some(&l) {
                    run.labels.push(l);
                }
            }
        }
        if down {
            run.was_down = true;
            run.withheld |= withholds_fire(ACTOR);
            if t > 1.8 {
                let pelvis = f.world_of(pelvis_node).1;
                let at = rig_fixture::read3(&f.e, ACTOR + 0x30);
                run.ref_off = run
                    .ref_off
                    .max(length([pelvis[0] - at[0], pelvis[1] - at[1], 0.0]));
            }
        } else if run.was_down && owns(ACTOR) && run.stood_again.is_none() {
            run.stood_again = Some(t);
        }
        if run.stood_again.is_some() {
            run.withheld_after |= withholds_fire(ACTOR);
        }
        // The bones with bodies or between them (the fingers and thumbs are the animation's, past
        // the hand's capsule: a palm pressed to the floor puts their tips a few units through it).
        let lowest = (1..f.nodes.len())
            .filter(|&i| {
                let n = &f.nodes[i].name;
                n.starts_with("Bip01 ") && !n.contains("Finger") && !n.contains("Thumb")
            })
            .map(|i| f.world_of(f.at[i]).1[2])
            .fold(f32::INFINITY, f32::min);
        if owns(ACTOR) {
            run.lowest = run.lowest.min(lowest);
        }
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
        if died && run.death.is_none() {
            if let Some(d) = deaths().first() {
                run.death = Some((d.1, d.2));
                for (node, body) in bodies_of(&mut f) {
                    let Some(k) = body_nodes.iter().position(|&i| f.at[i] == node) else {
                        continue;
                    };
                    if let Some(d) = drawn_at_death.get(k) {
                        let p = scale(rig_fixture::read3(&f.e, body + 0x120), rig_fixture::HK);
                        run.death_off = run.death_off.max(length(sub(p, d.1)));
                    }
                }
                break;
            }
        }
    }
    run.knock_state = f.e.mem.u8(rig_fixture::PROCESS_AT + 0x13c);
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    crate::gunplay::set_tuning(was);
    run
}

/// M6 on FNV's skeleton: a 4.35 m/s hit past any step is kept downed (no hand-over to the game's
/// ragdoll: knock state 0), drawn is simulated within 0.5 degrees and 0.5 units every downed
/// frame, the actor's reference follows the pelvis (within 15 units once settled), its shots are
/// withheld while it cannot aim (no target), the labels are logged as they change; asked to
/// stand (no target: the AI's choice), it stands by its balance again within 8 s and its shots
/// are not withheld then; no bone more than 3 units under the floor (the fingers and thumbs
/// aside: the animation's, past the hand's capsule).
#[test]
fn np_downed_on_fnvs_skeleton_stands_again() {
    let r = down_run(10.0, None);
    eprintln!(
        "RESULT np M6 downed on FNV's skeleton: knock state {}, labels {:?}, stood again at {:?} s, read-back {:.3} deg / {:.3} u, the reference at most {:.1} u from the pelvis, withheld {} (after {}), lowest {:.1} u",
        r.knock_state, r.labels, r.stood_again, r.readback.0, r.readback.1, r.ref_off, r.withheld, r.withheld_after, r.lowest
    );
    assert_eq!(r.knock_state, 0, "handed to the game's ragdoll");
    assert!(r.was_down, "never downed");
    assert!(
        r.readback.0 <= 0.5 && r.readback.1 <= 0.5,
        "{:?}",
        r.readback
    );
    assert!(
        r.ref_off < 15.0,
        "the reference {:.1} from the pelvis",
        r.ref_off
    );
    assert!(r.withheld && !r.withheld_after);
    assert!(r.labels.len() >= 2, "{:?}", r.labels);
    assert!(
        r.stood_again.is_some_and(|t| t < 8.0),
        "{:?}",
        r.stood_again
    );
    assert!(r.lowest > -3.0, "{:.1}", r.lowest);
}

/// Dying while downed: the game's ragdoll takes the body from the pose drawn (each of its 18
/// bodies placed within 1.5 units of it) and moving as New Physics' bodies were (each given its
/// velocity); New Physics lets go.
#[test]
fn np_dies_while_downed_on_fnvs_skeleton() {
    let r = down_run(4.0, Some(2.5));
    eprintln!(
        "RESULT np M6 dies while downed: labels {:?}, bodies placed and given velocities {:?}, at most {:.2} u off the pose drawn",
        r.labels, r.death, r.death_off
    );
    assert!(r.was_down);
    assert!(
        r.death.is_some_and(|d| d.0 == 18 && d.1 == 18),
        "{:?}",
        r.death
    );
    assert!(r.death_off < 1.5, "{:.2}", r.death_off);
    assert!(!owns(ACTOR));
}

// --- The in-game path (Patrick's clip 2026-11-17_18-59-52_7825515). -----------------------------

/// What a run on the in-game path did.
struct GamePath {
    claimed: bool,
    /// Every frame New Physics had it standing (the balance's phase), its catching steps, how
    /// long it held it (s), the spine top's peak off its animation (degrees), the most drawn lay
    /// off what was written (degrees, units), whether it ever fell.
    standing_throughout: bool,
    steps: u32,
    held: f32,
    spine_peak: f32,
    readback: (f32, f32),
    fell: bool,
    /// The frames from the hit to a knock state other than 0, the game's bodies' centre against
    /// New Physics' (a ratio), New Physics' let-gos.
    knocked_after: Option<u32>,
    centre_ratio: f32,
    let_gos: u32,
}

/// The run the game makes (`docs/GUNPLAY.md`, the clip): the actor at (-72152, 8282, 223); each
/// frame, the main thread's update (`main_thread`, the AI threads idle: `0x008c7aa0` false) and
/// the animation (0.3 units of jitter on the root each frame), then the draw's set-up
/// (`active_ragdoll::apply` with `0x008c7aa0` true: the AI threads use the task queue); the
/// game's frame seconds `dt`. The hit is the `HitMe` wrapper's: claim and hit in one frame, the
/// root moved 1 unit after it with its worlds not yet updated (the first frame's draw).
fn game_path(speed: f32, dt: f32, on: bool, keep_down: bool, seconds: f32) -> GamePath {
    use std::sync::atomic::{AtomicBool, Ordering};
    static AI_BUSY: AtomicBool = AtomicBool::new(false);
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let at: V3 = [-72152.0, 8282.0, 223.0];
    let mut f = Fixture::new(
        rig_fixture::fnv_skeleton(),
        Ground {
            collision: Some(at[2]),
            navmesh: at[2] - 8.0,
        },
    );
    f.make_character();
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    set_keep_down(keep_down);
    let was = crate::gunplay::tuning();
    crate::gunplay::set_tuning(crate::gunplay::Tuning {
        new_physics: on,
        ..was
    });
    rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, at);
    f.e.register_double(0x008c_7aa0, |_, _| {
        u32::from(AI_BUSY.load(Ordering::Relaxed)).into_ret()
    });
    f.e.register_double(0x0088_45a0, |_, _| 1u32.into_ret());
    f.e.register_double(crate::hit_force::KNOCK_EXPLOSION, |e, a| {
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
    let lets_before = let_gos();
    let mut run = GamePath {
        claimed: false,
        standing_throughout: true,
        steps: 0,
        held: 0.0,
        spine_peak: 0.0,
        readback: (0.0, 0.0),
        fell: false,
        knocked_after: None,
        centre_ratio: 0.0,
        let_gos: 0,
    };
    let frames = (seconds / dt).round() as u32;
    let hit_frame = (0.1 / dt).round() as u32;
    let mut seed = 0x1234_5678u32;
    let mut jitter = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let a = (seed >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU;
        [0.3 * a.cos(), 0.3 * a.sin(), 0.0]
    };
    for frame in 0..frames {
        f.e.mem.set_f32(0x011d_ea30, dt);
        // The main thread's update: the hand-overs waiting, then the animation.
        AI_BUSY.store(false, Ordering::Relaxed);
        main_thread(&mut f.e);
        let knock = f.e.mem.u8(rig_fixture::PROCESS_AT + 0x13c);
        if knock != 0 && run.knocked_after.is_none() && frame > hit_frame {
            run.knocked_after = Some(frame - hit_frame);
            // The game's bodies against New Physics' at the hand-over.
            let mass: f32 = model().bodies.iter().map(|b| b.mass).sum();
            let mut momentum = [0.0f32; 3];
            for (node, body) in bodies_of(&mut f) {
                let Some(k) = body_nodes.iter().position(|&i| f.at[i] == node) else {
                    continue;
                };
                let v = scale(rig_fixture::read3(&f.e, body + 0x10), rig_fixture::HK);
                momentum = add(momentum, scale(v, model().bodies[k].mass));
            }
            let centre = length(momentum) / mass;
            if let Some(fall) = falls().first() {
                run.centre_ratio = centre / fall.centre.max(1e-3);
            }
        }
        let reference = f.reference();
        rig_fixture::write3(
            &mut f.e,
            root + rig_fixture::LOCAL_TRANSLATION,
            add(reference, jitter()),
        );
        f.animate();
        keyframe(&mut f);
        let anim: Vec<(M3, V3)> = body_nodes.iter().map(|&i| f.world_of(f.at[i])).collect();
        if frame == hit_frame {
            // `HitMe`'s wrapper: claim, the game's hit, then the hit measured (one frame).
            run.claimed = claim(&mut f.e, ACTOR);
            hit(
                &mut f.e,
                ACTOR,
                speed * model().mass,
                add(anim[spine].1, [0.0, 8.0, 4.0]),
                [0.0, -1.0, 0.0],
            );
            // The root moved a unit since, its worlds not yet brought up to it.
            let r = rig_fixture::read3(&f.e, root + rig_fixture::LOCAL_TRANSLATION);
            rig_fixture::write3(
                &mut f.e,
                root + rig_fixture::LOCAL_TRANSLATION,
                add(r, [0.0, 1.0, 0.0]),
            );
        }
        // The draw's set-up, the AI threads mid-frame.
        AI_BUSY.store(true, Ordering::Relaxed);
        crate::active_ragdoll::apply(&mut f.e);
        if owns(ACTOR) {
            if let Some((p, _)) = drawn(ACTOR) {
                for (k, &i) in body_nodes.iter().enumerate() {
                    let w = f.world_of(f.at[i]);
                    run.readback.0 = run
                        .readback
                        .0
                        .max(ragdoll_core::np_skeleton::angle_between(&w.0, &p[k].0).to_degrees());
                    run.readback.1 = run.readback.1.max(length(sub(w.1, p[k].1)));
                }
                run.spine_peak = run.spine_peak.max(
                    ragdoll_core::np_skeleton::angle_between(&p[spine].0, &anim[spine].0)
                        .to_degrees(),
                );
            }
            if let Some((phase, steps, _)) = state(ACTOR) {
                run.steps = run.steps.max(steps);
                if phase != ragdoll_core::np_balance::Phase::Standing {
                    run.standing_throughout = false;
                }
                if phase == ragdoll_core::np_balance::Phase::Falling {
                    run.fell = true;
                }
            }
            run.held += dt;
        }
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
    }
    run.let_gos = let_gos() - lets_before;
    AI_BUSY.store(false, Ordering::Relaxed);
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    set_keep_down(true);
    crate::gunplay::set_tuning(was);
    run
}

/// The clip's hit: a 0.32 m/s chest hit (25.9 N s, a 9mm) at 250 and 60 frames a second on the
/// game's path: New Physics stands throughout (no step, never falling), the spine's top peaks
/// 5 to 8 degrees, it has the actor at least 0.5 s, and draws what it wrote (0.5 degrees, 0.5
/// units).
#[test]
fn np_game_path_a_light_hit_stands() {
    for dt in [0.004f32, 0.016] {
        let r = game_path(0.32, dt, true, true, 1.5);
        eprintln!(
            "RESULT np game path 0.32 m/s dt {dt}: standing throughout {}, steps {}, spine peak {:.1} deg, held {:.2} s, read-back {:.3} deg / {:.3} u, fell {}",
            r.standing_throughout, r.steps, r.spine_peak, r.held, r.readback.0, r.readback.1, r.fell
        );
        assert!(r.claimed);
        assert!(
            r.standing_throughout && !r.fell,
            "dt {dt}: not standing throughout"
        );
        assert_eq!(r.steps, 0, "dt {dt}");
        assert!(
            (5.0..=8.0).contains(&r.spine_peak),
            "dt {dt}: spine {:.1}",
            r.spine_peak
        );
        assert!(r.held >= 0.5, "dt {dt}: held {:.2} s", r.held);
        assert!(
            r.readback.0 <= 0.5 && r.readback.1 <= 0.5,
            "dt {dt}: read-back {:?}",
            r.readback
        );
    }
}

/// 0.8 m/s on the game's path: at least one catching step, no fall.
#[test]
fn np_game_path_a_medium_hit_steps() {
    for dt in [0.004f32, 0.016] {
        let r = game_path(0.8, dt, true, true, 2.5);
        eprintln!(
            "RESULT np game path 0.8 m/s dt {dt}: steps {}, fell {}, spine peak {:.1} deg, read-back {:.3} deg / {:.3} u",
            r.steps, r.fell, r.spine_peak, r.readback.0, r.readback.1
        );
        assert!(r.steps >= 1, "dt {dt}: no step");
        assert!(!r.fell, "dt {dt}: it fell");
        assert!(r.readback.0 <= 0.5 && r.readback.1 <= 0.5, "dt {dt}");
    }
}

/// 4.35 m/s on the game's path (M4's hand-over: the downed keeping off): the fall is handed to
/// the game's ragdoll on the main thread within 3 frames of the hit (knock state 2), the game's
/// bodies' centre within 2% of New Physics', and New Physics never lets it go.
#[test]
fn np_game_path_a_heavy_hit_falls_on_the_main_thread() {
    for dt in [0.004f32, 0.016] {
        let r = game_path(4.35, dt, true, false, 0.4);
        eprintln!(
            "RESULT np game path 4.35 m/s dt {dt}: knocked {:?} frames after the hit, the game's centre {:.3} of New Physics', let-gos {}",
            r.knocked_after, r.centre_ratio, r.let_gos
        );
        assert!(
            r.knocked_after.is_some_and(|k| k <= 3),
            "dt {dt}: {:?}",
            r.knocked_after
        );
        assert!(
            (r.centre_ratio - 1.0).abs() <= 0.02,
            "dt {dt}: {:.3}",
            r.centre_ratio
        );
        assert_eq!(r.let_gos, 0, "dt {dt}: let go");
    }
}

/// `bNewPhysics=0` on the game's path: nothing is taken.
#[test]
fn np_game_path_off_takes_nothing() {
    let r = game_path(0.32, 0.016, false, true, 0.3);
    assert!(!r.claimed && r.held == 0.0);
}

// --- The game's own update (Patrick's clip 2026-11-17_21-16-42_d02bbd1). -------------------------

/// What a run on the game's update did.
struct RealRun {
    claimed: bool,
    /// The most a body as drawn lay off New Physics' body (degrees, units), on any frame.
    drawn_off: (f32, f32),
    /// The most the body's centre moved against its animation (m/s), whether it fell or went down.
    rel_most: f32,
    fell: bool,
    /// Seconds from the hit until the pelvis as drawn is under 30 units over the floor; how far
    /// the pelvis as drawn travelled (flat, units) while the body as drawn stood upright (its
    /// spine's top within 20 degrees of its standing tilt, the pelvis over 50 units): an upright
    /// slide, not the fall's own motion.
    pelvis_down_after: Option<f32>,
    upright_slide: f32,
    /// How long after the hit the body was last drawn upright (s).
    upright_until: f32,
}

/// A hit of `speed` m/s on `body` with New Physics on the game's own update
/// (`Fixture::game_update`: no double for `00a59c60`, the body nodes' blend collision objects),
/// downed bodies kept (`KEEP_DOWN`, the game's), from a non-bind animated pose (the arms down, the
/// knees and elbows bent) with the feet 3 units under the reference's height; `dt` the game's
/// frame seconds, 0.3 units of jitter on the root, the AI threads busy at the draw's set-up.
fn real_run(speed: f32, body: &str, dt: f32, on: bool, seconds: f32) -> RealRun {
    use std::sync::atomic::{AtomicBool, Ordering};
    static AI_BUSY: AtomicBool = AtomicBool::new(false);
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut nodes = rig_fixture::fnv_skeleton();
    // Not the bind pose: each of these turned about its own z (degrees).
    for (name, deg) in [
        ("Bip01 L UpperArm", 50.0f32),
        ("Bip01 R UpperArm", 50.0),
        ("Bip01 L Forearm", 30.0),
        ("Bip01 R Forearm", 30.0),
        ("Bip01 L Calf", -12.0),
        ("Bip01 R Calf", -12.0),
        ("Bip01 Spine1", 6.0),
    ] {
        if let Some(n) = nodes.iter_mut().find(|n| n.name == name) {
            let (c, s) = deg.to_radians().sin_cos();
            let rz: M3 = [s, -c, 0.0, c, s, 0.0, 0.0, 0.0, 1.0];
            n.r = ragdoll_core::flinch::mul(&n.r, &rz);
        }
    }
    let at: V3 = [-72152.0, 8282.0, 223.0];
    let mut f = Fixture::new(
        nodes,
        Ground {
            collision: Some(at[2]),
            navmesh: at[2] - 8.0,
        },
    );
    f.game_update();
    f.make_character();
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    set_keep_down(true);
    let was = crate::gunplay::tuning();
    crate::gunplay::set_tuning(crate::gunplay::Tuning {
        new_physics: on,
        ..was
    });
    f.e.register_double(0x008c_7aa0, |_, _| {
        u32::from(AI_BUSY.load(Ordering::Relaxed)).into_ret()
    });
    f.e.register_double(0x0088_45a0, |_, _| 1u32.into_ret());
    f.e.register_double(crate::hit_force::KNOCK_EXPLOSION, |e, a| {
        e.mem.set_u8(a[0] + 0x13c, 2);
        0u32.into_ret()
    });
    f.e.register_double(0x00ca_8960, |e, a| {
        let p = rig_fixture::read3(e, a[1]);
        rig_fixture::write3(e, a[0] + 0x120, p);
        0u32.into_ret()
    });
    let body_nodes: Vec<usize> = model()
        .bodies
        .iter()
        .map(|b| f.nodes.iter().position(|n| n.name == b.node).unwrap())
        .collect();
    let struck = model().body(body).unwrap();
    let root = f.at[0];
    let pelvis = f.at[body_nodes[model().root]];
    // The feet 3 units under the reference's height: the root set below it by the animated
    // feet's lowest point and 3 more.
    rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, [0.0; 3]);
    f.animate();
    let anim0: Vec<(M3, V3)> = body_nodes.iter().map(|&i| f.world_of(f.at[i])).collect();
    let low = ragdoll_core::np_body::lowest_point(model(), &model().states_from_nodes(&anim0))
        * ragdoll_core::np_body::UNITS;
    let drop = [0.0, 0.0, -low - 3.0];
    rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, at);
    let mut run = RealRun {
        claimed: false,
        drawn_off: (0.0, 0.0),
        rel_most: 0.0,
        fell: false,
        pelvis_down_after: None,
        upright_slide: 0.0,
        upright_until: 0.0,
    };
    let frames = (seconds / dt).round() as u32;
    let hit_frame = (0.1 / dt).round() as u32;
    let mut seed = 0x2468_ace1u32;
    let mut jitter = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let a = (seed >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU;
        [0.3 * a.cos(), 0.3 * a.sin(), 0.0]
    };
    let mut last_pelvis: Option<V3> = None;
    let spine_top = f.at[body_nodes[model().body("Bip01 Spine2").unwrap()]];
    let up_of = |r: &M3| r[8].clamp(-1.0, 1.0).acos().to_degrees();
    let spine_up = up_of(&f.world_of(spine_top).0);
    for frame in 0..frames {
        f.e.mem.set_f32(0x011d_ea30, dt);
        AI_BUSY.store(false, Ordering::Relaxed);
        main_thread(&mut f.e);
        let reference = f.reference();
        rig_fixture::write3(
            &mut f.e,
            root + rig_fixture::LOCAL_TRANSLATION,
            add(add(reference, drop), jitter()),
        );
        f.animate();
        let anim: Vec<(M3, V3)> = body_nodes.iter().map(|&i| f.world_of(f.at[i])).collect();
        if frame == hit_frame {
            run.claimed = claim(&mut f.e, ACTOR);
            let point = add(anim[struck].1, [0.0, 6.0, 2.0]);
            hit(
                &mut f.e,
                ACTOR,
                speed * model().mass,
                point,
                [0.0, -1.0, 0.0],
            );
        }
        AI_BUSY.store(true, Ordering::Relaxed);
        crate::active_ragdoll::apply(&mut f.e);
        if owns(ACTOR) {
            if let Some((p, _)) = drawn(ACTOR) {
                for (k, &i) in body_nodes.iter().enumerate() {
                    let w = f.world_of(f.at[i]);
                    run.drawn_off.0 = run
                        .drawn_off
                        .0
                        .max(ragdoll_core::np_skeleton::angle_between(&w.0, &p[k].0).to_degrees());
                    run.drawn_off.1 = run.drawn_off.1.max(length(sub(w.1, p[k].1)));
                }
            }
            if let Some(v) = rel_speed(ACTOR) {
                run.rel_most = run.rel_most.max(v);
            }
            if let Some((phase, _, _)) = state(ACTOR) {
                run.fell |= phase == ragdoll_core::np_balance::Phase::Falling;
            }
            run.fell |= label_of(ACTOR).is_some();
        }
        if frame > hit_frame {
            let ref_now = f.reference();
            let p = f.world_of(pelvis).1;
            let height = p[2] - ref_now[2];
            if height < 30.0 && run.pelvis_down_after.is_none() {
                run.pelvis_down_after = Some((frame - hit_frame) as f32 * dt);
            }
            // Upright: the spine's top as drawn within 20 degrees of its standing tilt.
            let tilt = (up_of(&f.world_of(spine_top).0) - spine_up).abs();
            if let Some(was) = last_pelvis {
                if height > 50.0 && tilt < 20.0 {
                    run.upright_slide += length([p[0] - was[0], p[1] - was[1], 0.0]);
                    run.upright_until = (frame - hit_frame) as f32 * dt;
                }
            }
            last_pelvis = Some(p);
        }
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
    }
    AI_BUSY.store(false, Ordering::Relaxed);
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    crate::gunplay::set_tuning(was);
    run
}

/// The clip's 9mm (0.32 m/s) on the chest and on the left upper arm, on the game's own update:
/// each frame each body is drawn within 1 degree and 1 unit of New Physics' body, the body's
/// centre never moves against its animation more than 10% faster than the hit made it (the
/// ground's reaction as a struck arm swings the body round its feet adds up to 6%; the clip's
/// take kick took 0.32 m/s to 1.04, d02bbd13's fixture to 0.39), no fall.
#[test]
fn np_real_update_a_light_hit_is_drawn() {
    for body in ["Bip01 Spine2", "Bip01 L UpperArm"] {
        for dt in [0.004f32, 0.016] {
            let r = real_run(0.32, body, dt, true, 1.2);
            eprintln!(
                "RESULT np real update 0.32 m/s {body} dt {dt}: drawn off the body at most {:.3} deg / {:.3} u, centre against the animation at most {:.3} m/s, fell {}",
                r.drawn_off.0, r.drawn_off.1, r.rel_most, r.fell
            );
            assert!(r.claimed);
            assert!(
                r.drawn_off.0 <= 1.0 && r.drawn_off.1 <= 1.0,
                "{body} dt {dt}: drawn off {:?}",
                r.drawn_off
            );
            assert!(
                r.rel_most <= 0.32 * 1.10,
                "{body} dt {dt}: {:.3} m/s",
                r.rel_most
            );
            assert!(!r.fell, "{body} dt {dt}: fell");
        }
    }
}

/// The clip's rifle (4.83 m/s) on the game's own update, downed kept: the pelvis as drawn is
/// under 30 units over the floor within 0.6 s of the hit, and it never slides upright (the clip
/// slid 270 units standing): drawn upright (the spine's top within 20 degrees of its standing
/// tilt) no later than 0.4 s after the hit, and meanwhile the pelvis as drawn travels no further
/// than the hit's speed carries it (the fall's own motion).
#[test]
fn np_real_update_a_heavy_hit_goes_down() {
    for dt in [0.004f32, 0.016] {
        let r = real_run(4.83, "Bip01 Spine2", dt, true, 1.2);
        eprintln!(
            "RESULT np real update 4.83 m/s dt {dt}: pelvis under 30 u after {:?} s, the pelvis travelled {:.1} u while drawn upright (upright until {:.2} s), drawn off the body at most {:.3} deg / {:.3} u",
            r.pelvis_down_after, r.upright_slide, r.upright_until, r.drawn_off.0, r.drawn_off.1
        );
        assert!(
            r.pelvis_down_after.is_some_and(|t| t <= 0.6),
            "dt {dt}: {:?}",
            r.pelvis_down_after
        );
        assert!(
            r.upright_until <= 0.4 && r.upright_slide <= 4.83 * 70.0 * r.upright_until.max(dt),
            "dt {dt}: slid {:.1}",
            r.upright_slide
        );
    }
}

/// `bNewPhysics=0` on the game's own update: nothing is taken.
#[test]
fn np_real_update_off_takes_nothing() {
    let r = real_run(0.32, "Bip01 Spine2", 0.016, false, 0.3);
    assert!(!r.claimed && r.drawn_off == (0.0, 0.0));
}

/// The old system's own fallback on the same fixture (the game's update, the blend collision
/// objects): its write is not taken by the update, and it sets the worlds directly
/// (`[target drawn] ... worlds set directly`).
#[test]
fn old_reactions_set_the_worlds_directly_on_the_real_update() {
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
    f.game_update();
    crate::active_ragdoll::clear();
    let was = crate::gunplay::tuning();
    crate::gunplay::set_tuning(crate::gunplay::Tuning {
        new_physics: false,
        ..was
    });
    crate::room_events::set_on(true);
    let _ = crate::room_events::take();
    f.animate();
    crate::active_ragdoll::apply(&mut f.e);
    crate::active_ragdoll::after_draw(&mut f.e);
    crate::active_ragdoll::unwrite(&mut f.e);
    let chest = f.world("Bip01 Spine2").unwrap().1;
    crate::active_ragdoll::hit(
        &mut f.e,
        ACTOR,
        0.8 * 80.0,
        80.0,
        add(chest, [0.0, 8.0, 4.0]),
        [0.0, -1.0, 0.0],
        false,
    );
    let mut said = false;
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(16));
        f.animate();
        crate::active_ragdoll::apply(&mut f.e);
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
        said |= crate::room_events::take()
            .iter()
            .any(|l| l.contains("[target drawn]") && l.contains("worlds set directly"));
    }
    crate::room_events::set_on(false);
    crate::active_ragdoll::clear();
    crate::gunplay::set_tuning(was);
    eprintln!("RESULT old reactions on the real update: worlds set directly said {said}");
    assert!(said, "the old system's fallback did not run");
}

/// What a run on the game's own update with a walking, turning animation and a writer of the
/// animation's locals drew, as the world pass saw it.
struct WalkRun {
    claimed: bool,
    /// The most a body as the world pass saw it lay off New Physics' body (degrees, units), on
    /// any frame; the frames it was drawn nearer its animation than the body (the body off its
    /// animation by over 2 degrees or 1 unit).
    drawn_off: (f32, f32),
    drawn_as_animation: u32,
    fell: bool,
    /// The most the floor carried in a frame, in body weights (the frame's mean).
    floor_most: f32,
    /// Seconds from the hit until the pelvis as drawn is under 30 units over the floor, and the
    /// most it was drawn over the floor after the hit (units).
    pelvis_down_after: Option<f32>,
    pelvis_high: f32,
    /// The animated feet's most lift (units) and the animation's speed (m/s).
    lift: f32,
    anim_speed: f32,
}

/// How far the gait's swinging foot is lifted (units), its speed (m/s) and turn (degrees a
/// second), its period (s).
const WALK_LIFT: f32 = 5.0;
const WALK_SPEED: f32 = 0.15;
const WALK_TURN: f32 = 10.0;
const WALK_PERIOD: f32 = 1.2;
const WALK_BOB: f32 = 2.5;

/// A hit of `speed` m/s on `body` with New Physics on the game's own update
/// (`Fixture::game_update`, the blend collision objects, downed kept), the animation walking at
/// [`WALK_SPEED`] and turning at [`WALK_TURN`] with each foot lifted [`WALK_LIFT`] units in turn
/// and the whole animation bobbing [`WALK_BOB`] units up and down twice a stride, the stance's
/// feet `under` units under the reference's height; `dt` the game's frame seconds.
/// Every other frame, between the write and the world pass, a writer (the AI threads' animation
/// pass) sets the animation's locals on every node and works the worlds out from them. Each frame
/// the bodies' nodes are measured where the world pass draws them (after
/// `crate::active_ragdoll::read_drawn`, what the world pass runs first).
fn walk_run(speed: f32, body: &str, dt: f32, on: bool, seconds: f32, under: f32) -> WalkRun {
    use std::sync::atomic::{AtomicBool, Ordering};
    static AI_BUSY: AtomicBool = AtomicBool::new(false);
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut nodes = rig_fixture::fnv_skeleton();
    let turn_z = |r: &M3, deg: f32| {
        let (s, c) = deg.to_radians().sin_cos();
        let rz: M3 = [c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0];
        ragdoll_core::flinch::mul(r, &rz)
    };
    for (name, deg) in [
        ("Bip01 L UpperArm", 50.0f32),
        ("Bip01 R UpperArm", 50.0),
        ("Bip01 L Forearm", 30.0),
        ("Bip01 R Forearm", 30.0),
        ("Bip01 L Calf", -12.0),
        ("Bip01 R Calf", -12.0),
        ("Bip01 Spine1", 6.0),
    ] {
        if let Some(n) = nodes.iter_mut().find(|n| n.name == name) {
            n.r = turn_z(&n.r, deg);
        }
    }
    let at: V3 = [-72152.0, 8282.0, 223.0];
    let mut f = Fixture::new(
        nodes,
        Ground {
            collision: Some(at[2]),
            navmesh: at[2] - 8.0,
        },
    );
    f.game_update();
    f.make_character();
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    set_keep_down(true);
    let was = crate::gunplay::tuning();
    crate::gunplay::set_tuning(crate::gunplay::Tuning {
        new_physics: on,
        ..was
    });
    f.e.register_double(0x008c_7aa0, |_, _| {
        u32::from(AI_BUSY.load(Ordering::Relaxed)).into_ret()
    });
    f.e.register_double(0x0088_45a0, |_, _| 1u32.into_ret());
    f.e.register_double(crate::hit_force::KNOCK_EXPLOSION, |e, a| {
        e.mem.set_u8(a[0] + 0x13c, 2);
        0u32.into_ret()
    });
    f.e.register_double(0x00ca_8960, |e, a| {
        let p = rig_fixture::read3(e, a[1]);
        rig_fixture::write3(e, a[0] + 0x120, p);
        0u32.into_ret()
    });
    let body_nodes: Vec<usize> = model()
        .bodies
        .iter()
        .map(|b| f.nodes.iter().position(|n| n.name == b.node).unwrap())
        .collect();
    let struck = model().body(body).unwrap();
    let root = f.at[0];
    let pelvis = f.at[body_nodes[model().root]];
    let feet = ["Bip01 L Foot", "Bip01 R Foot"].map(|n| model().body(n).unwrap());
    let legs = [
        ["Bip01 L Thigh", "Bip01 L Calf"],
        ["Bip01 R Thigh", "Bip01 R Calf"],
    ]
    .map(|p| p.map(|n| f.nodes.iter().position(|x| x.name == n).unwrap()));
    let rest: Vec<M3> = f.nodes.iter().map(|n| n.r).collect();
    let k = ragdoll_core::np_body::UNITS;
    let foot_low = |f: &Fixture, side: usize| {
        let b = feet[side];
        let (r, t) = f.world_of(f.at[body_nodes[b]]);
        let def = &model().bodies[b];
        def.ends
            .iter()
            .map(|e| add(t, apply_m(&r, scale(*e, k)))[2] - def.radius * k)
            .fold(f32::INFINITY, f32::min)
    };
    // Each leg's swing: the thigh forward by `a`, the calf back by twice it.
    let pose_leg = |f: &mut Fixture, side: usize, a: f32| {
        let [thigh, calf] = legs[side];
        f.nodes[thigh].r = turn_z(&rest[thigh], a);
        f.nodes[calf].r = turn_z(&rest[calf], -2.0 * a);
    };
    rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, [0.0; 3]);
    f.animate();
    let low0 = foot_low(&f, 0);
    // The swing's angle that lifts the foot WALK_LIFT units (its sign the one that lifts it).
    let mut swing = 0.0f32;
    for sign in [1.0f32, -1.0] {
        for i in 1..=80 {
            let a = sign * i as f32 * 0.5;
            pose_leg(&mut f, 0, a);
            f.animate();
            if foot_low(&f, 0) - low0 >= WALK_LIFT {
                swing = a;
                break;
            }
        }
        if swing != 0.0 {
            break;
        }
    }
    pose_leg(&mut f, 0, 0.0);
    f.animate();
    assert!(swing != 0.0, "no swing lifts the foot");
    let anim0: Vec<(M3, V3)> = body_nodes.iter().map(|&i| f.world_of(f.at[i])).collect();
    let low = ragdoll_core::np_body::lowest_point(model(), &model().states_from_nodes(&anim0)) * k;
    let drop = [0.0, 0.0, -low - under];
    let floor = at[2] - under;
    rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, at);
    let mut run = WalkRun {
        claimed: false,
        drawn_off: (0.0, 0.0),
        drawn_as_animation: 0,
        fell: false,
        floor_most: 0.0,
        pelvis_down_after: None,
        pelvis_high: 0.0,
        lift: 0.0,
        anim_speed: 0.0,
    };
    let frames = (seconds / dt).round() as u32;
    let hit_frame = (0.4 / dt).round() as u32;
    let weight = model().mass * 9.81;
    let mut yaw = 0.0f32;
    let mut last_root: Option<V3> = None;
    for frame in 0..frames {
        let t = frame as f32 * dt;
        f.e.mem.set_f32(0x011d_ea30, dt);
        AI_BUSY.store(false, Ordering::Relaxed);
        main_thread(&mut f.e);
        // The walk: the reference carried on along its facing and turned, each foot lifted in
        // turn.
        yaw += WALK_TURN * dt;
        let (s, c) = yaw.to_radians().sin_cos();
        let mut reference = f.reference();
        if frame > 0 {
            reference = add(
                reference,
                [-s * WALK_SPEED * k * dt, c * WALK_SPEED * k * dt, 0.0],
            );
            rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, reference);
        }
        let phase = (std::f32::consts::TAU * t / WALK_PERIOD).sin();
        pose_leg(&mut f, 0, swing * phase.max(0.0).sqrt());
        pose_leg(&mut f, 1, swing * (-phase).max(0.0).sqrt());
        let yaw_m: M3 = [c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0];
        rig_fixture::write9(&mut f.e, root + rig_fixture::LOCAL_ROTATION, &yaw_m);
        // The gait's bob: the whole animation WALK_BOB up and down twice a stride (both feet
        // off the stance's height).
        let bob = WALK_BOB * (2.0 * std::f32::consts::TAU * t / WALK_PERIOD).cos();
        let root_at = add(add(reference, drop), [0.0, 0.0, bob]);
        rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, root_at);
        f.animate();
        for side in 0..2 {
            run.lift = run.lift.max(foot_low(&f, side) - (low0 + root_at[2]));
        }
        if let Some(was) = last_root.filter(|_| frame < hit_frame) {
            let flat = sub(root_at, was);
            run.anim_speed = run.anim_speed.max(length([flat[0], flat[1], 0.0]) / k / dt);
        }
        last_root = Some(root_at);
        let anim: Vec<(M3, V3)> = body_nodes.iter().map(|&i| f.world_of(f.at[i])).collect();
        let locals: Vec<(M3, V3)> =
            f.at.iter()
                .map(|&n| {
                    (
                        rig_fixture::read9(&f.e, n + rig_fixture::LOCAL_ROTATION),
                        rig_fixture::read3(&f.e, n + rig_fixture::LOCAL_TRANSLATION),
                    )
                })
                .collect();
        if frame == hit_frame {
            run.claimed = claim(&mut f.e, ACTOR);
            let point = add(anim[struck].1, [0.0, 6.0, 2.0]);
            hit(
                &mut f.e,
                ACTOR,
                speed * model().mass,
                point,
                [0.0, -1.0, 0.0],
            );
        }
        AI_BUSY.store(true, Ordering::Relaxed);
        crate::active_ragdoll::apply(&mut f.e);
        // The writer: every other frame the animation's locals set again and the worlds worked
        // out from them, after the write and before the world pass.
        if frame % 2 == 1 {
            for (i, &n) in f.at.iter().enumerate().skip(1) {
                rig_fixture::write9(&mut f.e, n + rig_fixture::LOCAL_ROTATION, &locals[i].0);
                rig_fixture::write3(&mut f.e, n + rig_fixture::LOCAL_TRANSLATION, locals[i].1);
            }
            f.update();
        }
        // The world pass.
        crate::active_ragdoll::read_drawn(&mut f.e);
        if owns(ACTOR) {
            if let Some((p, _)) = drawn(ACTOR) {
                let (mut most, mut as_anim) = ((0.0f32, 0.0f32), false);
                for (b, &i) in body_nodes.iter().enumerate() {
                    let w = f.world_of(f.at[i]);
                    let deg = ragdoll_core::np_skeleton::angle_between(&w.0, &p[b].0).to_degrees();
                    let u = length(sub(w.1, p[b].1));
                    most = (most.0.max(deg), most.1.max(u));
                    let sim_anim = (
                        ragdoll_core::np_skeleton::angle_between(&p[b].0, &anim[b].0).to_degrees(),
                        length(sub(p[b].1, anim[b].1)),
                    );
                    let drawn_anim = (
                        ragdoll_core::np_skeleton::angle_between(&w.0, &anim[b].0).to_degrees(),
                        length(sub(w.1, anim[b].1)),
                    );
                    if (sim_anim.0 > 2.0 || sim_anim.1 > 1.0)
                        && drawn_anim.0 + drawn_anim.1 < deg + u
                    {
                        as_anim = true;
                    }
                }
                run.drawn_off = (run.drawn_off.0.max(most.0), run.drawn_off.1.max(most.1));
                run.drawn_as_animation += u32::from(as_anim);
            }
            if let Some(n) = floor_force(ACTOR) {
                run.floor_most = run.floor_most.max(n / weight);
            }
            if let Some((phase, _, _)) = state(ACTOR) {
                run.fell |= phase == ragdoll_core::np_balance::Phase::Falling;
            }
            run.fell |= label_of(ACTOR).is_some();
        }
        if frame > hit_frame {
            let height = f.world_of(pelvis).1[2] - floor;
            run.pelvis_high = run.pelvis_high.max(height);
            if height < 30.0 && run.pelvis_down_after.is_none() {
                run.pelvis_down_after = Some((frame - hit_frame) as f32 * dt);
            }
        }
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
    }
    AI_BUSY.store(false, Ordering::Relaxed);
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    crate::gunplay::set_tuning(was);
    run
}

/// Patrick's 803260d clip's 10mm (0.52 m/s) on the chest, the head and the right thigh, the
/// animation walking and turning, a writer of its locals every other frame, dt 0.016 and 0.033,
/// the stance's feet 3 and 7 units under the reference (where the floor taken from the animated
/// feet switched to the reference's and back):
/// no fall; every frame each body drawn at the world pass within 1 degree and 1 unit of New
/// Physics' body (never drawn as the animation); the floor never carrying 3 body weights.
#[test]
fn np_walking_a_10mm_hit_is_drawn_and_stands() {
    let mut failed = Vec::new();
    for body in ["Bip01 Spine2", "Bip01 Head", "Bip01 R Thigh"] {
        for (dt, under) in [(0.016f32, 3.0f32), (0.033, 3.0), (0.016, 7.0), (0.033, 7.0)] {
            let r = walk_run(0.52, body, dt, true, 2.4, under);
            eprintln!(
                "RESULT np walking 0.52 m/s {body} dt {dt} feet {under} u under: drawn off the body at the world pass at most {:.3} deg / {:.3} u, {} frames drawn as the animation, floor at most {:.2} body weights, fell {} (feet lifted {:.1} u, the animation at {:.2} m/s)",
                r.drawn_off.0, r.drawn_off.1, r.drawn_as_animation, r.floor_most, r.fell, r.lift, r.anim_speed
            );
            assert!(r.claimed);
            assert!((r.lift - WALK_LIFT).abs() < 1.0, "lift {}", r.lift);
            if r.drawn_off.0 > 1.0 || r.drawn_off.1 > 1.0 || r.drawn_as_animation > 0 {
                failed.push(format!(
                    "{body} dt {dt} under {under}: drawn off {:?}, {} as the animation",
                    r.drawn_off, r.drawn_as_animation
                ));
            }
            if r.floor_most >= 3.0 {
                failed.push(format!(
                    "{body} dt {dt} under {under}: floor {:.2} body weights",
                    r.floor_most
                ));
            }
            if r.fell {
                failed.push(format!("{body} dt {dt} under {under}: fell"));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The clip's rifle (4.29 m/s) on the chest with the same walk, writer and stances: down (the pelvis as
/// drawn under 30 units over the floor) within 0.6 s, and the pelvis as drawn never over 80
/// units over the floor (the clip's launched him 206 units up).
#[test]
fn np_walking_a_rifle_hit_goes_down_and_stays_on_the_floor() {
    let mut failed = Vec::new();
    for (dt, under) in [(0.016f32, 3.0f32), (0.033, 3.0), (0.016, 7.0), (0.033, 7.0)] {
        let r = walk_run(4.29, "Bip01 Spine2", dt, true, 2.4, under);
        eprintln!(
            "RESULT np walking 4.29 m/s dt {dt} feet {under} u under: pelvis under 30 u after {:?} s, at most {:.1} u over the floor, drawn off the body at the world pass at most {:.3} deg / {:.3} u, {} frames drawn as the animation, floor at most {:.2} body weights",
            r.pelvis_down_after, r.pelvis_high, r.drawn_off.0, r.drawn_off.1, r.drawn_as_animation, r.floor_most
        );
        if !r.pelvis_down_after.is_some_and(|t| t <= 0.6) {
            failed.push(format!(
                "dt {dt} under {under}: down after {:?}",
                r.pelvis_down_after
            ));
        }
        if r.pelvis_high > 80.0 {
            failed.push(format!(
                "dt {dt} under {under}: pelvis {:.1} u up",
                r.pelvis_high
            ));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// `bNewPhysics=0` with the walk and the writer: nothing is taken.
#[test]
fn np_walking_off_takes_nothing() {
    let r = walk_run(0.52, "Bip01 Spine2", 0.033, false, 0.8, 3.0);
    assert!(!r.claimed && r.drawn_off == (0.0, 0.0) && r.floor_most == 0.0);
}
