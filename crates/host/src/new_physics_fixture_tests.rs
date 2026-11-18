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
        // The main thread's frame (the game's: its safe point for New Physics' moves).
        main_thread(&mut f.e);
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
        // The world pass (what the frame draws: New Physics sets the bodies' worlds there).
        crate::active_ragdoll::read_drawn(&mut f.e);
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
            body_off = body_off.max(length(sub(p, h.2)));
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
    /// How far the reference moved while downed (units).
    ref_moved_down: f32,
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
        ref_moved_down: 0.0,
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
    let mut down_ref: Option<V3> = None;
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
        // The world pass (what the frame draws: New Physics sets the bodies' worlds there).
        crate::active_ragdoll::read_drawn(&mut f.e);
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
            // New Physics owns the reference: it does not move while downed.
            let at = rig_fixture::read3(&f.e, ACTOR + 0x30);
            let from = *down_ref.get_or_insert(at);
            run.ref_moved_down = run.ref_moved_down.max(length(sub(at, from)));
        } else if run.was_down && owns(ACTOR) && run.stood_again.is_none() {
            run.stood_again = Some(t);
        }
        // Stood again: the actor moved to where the body is (the rise's end).
        if run.stood_again.is_some_and(|s| t > s + 0.3) && owns(ACTOR) {
            let pelvis = f.world_of(pelvis_node).1;
            let at = rig_fixture::read3(&f.e, ACTOR + 0x30);
            run.ref_off = run
                .ref_off
                .max(length([pelvis[0] - at[0], pelvis[1] - at[1], 0.0]));
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
/// frame, the actor's reference stays put while downed (New Physics owns it: under 1 unit) and
/// is where the pelvis is once it stands again (within 15 units; it followed the pelvis every
/// downed frame before the tester's 08:13 clip, gliding 14 to 53 units a second), its shots are
/// withheld while it cannot aim (no target), the labels are logged as they change; asked to
/// stand (no target: the AI's choice), it stands by its balance again within 8 s and its shots
/// are not withheld then; no bone more than 3 units under the floor (the fingers and thumbs
/// aside: the animation's, past the hand's capsule).
#[test]
fn np_downed_on_fnvs_skeleton_stands_again() {
    let r = down_run(10.0, None);
    eprintln!(
        "RESULT np M6 downed on FNV's skeleton: knock state {}, labels {:?}, stood again at {:?} s, read-back {:.3} deg / {:.3} u, the reference moved {:.2} u while downed and at most {:.1} u from the pelvis once standing again, withheld {} (after {}), lowest {:.1} u",
        r.knock_state, r.labels, r.stood_again, r.readback.0, r.readback.1, r.ref_moved_down, r.ref_off, r.withheld, r.withheld_after, r.lowest
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
    assert!(
        r.ref_moved_down < 1.0,
        "the reference moved {:.1} while downed",
        r.ref_moved_down
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
        // The world pass (what the frame draws: New Physics sets the bodies' worlds there).
        crate::active_ragdoll::read_drawn(&mut f.e);
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
        // The world pass (what the frame draws: New Physics sets the bodies' worlds there).
        crate::active_ragdoll::read_drawn(&mut f.e);
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
        // The layered reactions act only with New Physics on (for an actor it does not take: this
        // one is no character, `Fixture::make_character` not called).
        new_physics: true,
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
    /// The catching steps, the most the hips were lowered for a step (m), the outcome's lines.
    steps: u32,
    lower_most: f32,
    outcomes: Vec<String>,
    /// While downed with a combat target: the target's direction and the aim's angle off it the
    /// last frame (`new_physics::aim_of`).
    aim: Option<(Option<V3>, f32)>,
    aim_from: V3,
}

/// A run's set-up ([`walk_run_with`]).
struct Walk<'a> {
    speed: f32,
    body: &'a str,
    /// The game's frame seconds: each frame drawn between these (the same for a fixed rate).
    dt: (f32, f32),
    on: bool,
    seconds: f32,
    under: f32,
    /// The animation walks and turns (else it stands, its feet still).
    walking: bool,
    /// The writer of the animation's locals every this many frames.
    writer_every: u32,
    /// A combat target where the actor's vtable's `GetCombatTarget` answers it (this far from
    /// the actor), the actor's `+0x42c` field holding 480.0 (`0x43f00000`) as in Patrick's
    /// tester logs.
    target: Option<V3>,
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
    walk_run_with(&Walk {
        speed,
        body,
        dt: (dt, dt),
        on,
        seconds,
        under,
        walking: true,
        writer_every: 2,
        target: None,
    })
}

/// [`walk_run`] as `cfg` sets it up.
fn walk_run_with(cfg: &Walk) -> WalkRun {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (speed, body, on, seconds, under) = (cfg.speed, cfg.body, cfg.on, cfg.seconds, cfg.under);
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
        steps: 0,
        lower_most: 0.0,
        outcomes: Vec::new(),
        aim: None,
        aim_from: [0.0; 3],
    };
    if let Some(off) = cfg.target {
        // The target: a reference of its own (form id, position) the form table knows; the
        // actor's vtable's `GetCombatTarget` answers it; the field at +0x42c holds a float.
        const TARGET: u32 = 0x0300_0000;
        const TARGET_ID: u32 = 0x0012_3999;
        const GET_TARGET: u32 = 0x7200_0028;
        f.e.mem.map(TARGET, 0x1000);
        f.e.mem.set_u32(TARGET + 0x0c, TARGET_ID);
        rig_fixture::write3(&mut f.e, TARGET + rig_fixture::REFR_POSITION, add(at, off));
        f.e.register_double(GET_TARGET, |_, _| TARGET.into_ret());
        let vtable = f.e.mem.u32(ACTOR);
        f.e.mem.set_u32(vtable + 0x42c, GET_TARGET);
        f.e.mem.set_u32(ACTOR + 0x42c, 0x43f0_0000);
        f.e.register_double(0x0048_39c0, |_, a| {
            match a[0] {
                rig_fixture::ACTOR_ID => ACTOR,
                TARGET_ID => TARGET,
                _ => 0,
            }
            .into_ret()
        });
    }
    room_events::set_on(true);
    let _ = room_events::take();
    let weight = model().mass * 9.81;
    let mut yaw = 0.0f32;
    let mut last_root: Option<V3> = None;
    let mut seed = 0x1357_9bdfu32;
    let mut t = 0.0f32;
    let mut hit_frame = u32::MAX;
    let mut hit_t = 0.0f32;
    let mut frame = 0u32;
    while t < seconds {
        let dt = if cfg.dt.0 == cfg.dt.1 {
            cfg.dt.0
        } else {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            cfg.dt.0 + (cfg.dt.1 - cfg.dt.0) * ((seed >> 8) as f32 / (1u32 << 24) as f32)
        };
        t += dt;
        if hit_frame == u32::MAX && t >= 0.4 {
            hit_frame = frame;
            hit_t = t;
        }
        f.e.mem.set_f32(0x011d_ea30, dt);
        AI_BUSY.store(false, Ordering::Relaxed);
        main_thread(&mut f.e);
        // The walk: the reference carried on along its facing and turned, each foot lifted in
        // turn.
        let walk = if cfg.walking { 1.0 } else { 0.0 };
        yaw += WALK_TURN * dt * walk;
        let (s, c) = yaw.to_radians().sin_cos();
        let mut reference = f.reference();
        if frame > 0 && walk > 0.0 {
            reference = add(
                reference,
                [-s * WALK_SPEED * k * dt, c * WALK_SPEED * k * dt, 0.0],
            );
            rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, reference);
        }
        let phase = walk * (std::f32::consts::TAU * t / WALK_PERIOD).sin();
        pose_leg(&mut f, 0, swing * phase.max(0.0).sqrt());
        pose_leg(&mut f, 1, swing * (-phase).max(0.0).sqrt());
        let yaw_m: M3 = [c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0];
        rig_fixture::write9(&mut f.e, root + rig_fixture::LOCAL_ROTATION, &yaw_m);
        // The gait's bob: the whole animation WALK_BOB up and down twice a stride (both feet
        // off the stance's height).
        let bob = walk * WALK_BOB * (2.0 * std::f32::consts::TAU * t / WALK_PERIOD).cos();
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
        if frame % cfg.writer_every == cfg.writer_every - 1 {
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
            if let Some((_, steps, _)) = state(ACTOR) {
                run.steps = run.steps.max(steps);
            }
            if let Some(l) = field(&sample_field(ACTOR), "lower") {
                run.lower_most = run.lower_most.max(l);
            }
            if let Some(a) = aim_of(ACTOR) {
                run.aim = Some(a);
                run.aim_from = f.reference();
            }
            if std::env::var_os("NP_TRACE").is_some() {
                eprintln!(
                    "TRACE t {t:.3} dt {dt:.3} {:?} floor {:.2} {}",
                    state(ACTOR).map(|s| (s.0, s.1)),
                    floor_force(ACTOR).unwrap_or(0.0) / weight,
                    sample_field(ACTOR)
                );
            }
        }
        if frame > hit_frame && hit_frame != u32::MAX {
            let height = f.world_of(pelvis).1[2] - floor;
            run.pelvis_high = run.pelvis_high.max(height);
            if height < 30.0 && run.pelvis_down_after.is_none() {
                run.pelvis_down_after = Some(t - hit_t);
            }
        }
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
        for l in room_events::take() {
            if let Some(i) = l.find("[np outcome]") {
                run.outcomes
                    .push(l[i..].trim_end_matches(['"', '}']).to_string());
            }
        }
        frame += 1;
    }
    room_events::set_on(false);
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

/// Patrick's tester crashes on b507e997 (20:58, 23:01: a read at 43f00030 in the draw): downed,
/// the combat target was read from the actor's `+0x42c` as a field, which held 480.0
/// (`0x43f00000`), and its position past the end of memory. A rifle hit downs the actor on the
/// game's own update with that field and the vtable's `GetCombatTarget` answering a real target
/// 300 units off: no panic, and the downed AI's aim reads that target.
#[test]
fn np_downed_aims_at_the_vtables_combat_target() {
    let off = [300.0, 40.0, 0.0];
    let r = walk_run_with(&Walk {
        speed: 4.29,
        body: "Bip01 Spine2",
        dt: (0.016, 0.016),
        on: true,
        seconds: 2.5,
        under: 3.0,
        walking: false,
        writer_every: 2,
        target: Some(off),
    });
    eprintln!(
        "RESULT np downed aims at the vtable's combat target: fell {}, aim {:?}",
        r.fell, r.aim
    );
    assert!(r.fell, "not downed");
    let (to, aim_off) = r.aim.expect("no downed aim read");
    let to = to.expect("no target read");
    let want = sub(add([-72152.0, 8282.0, 223.0], off), r.aim_from);
    assert!(length(sub(to, want)) < 1.0, "target {to:?}, want {want:?}");
    assert!(
        aim_off.is_finite() && (0.0..=180.0).contains(&aim_off),
        "{aim_off}"
    );
}

/// The testers' 9mm (0.32 m/s) on the chest: it must not walk itself into a fall. The game's
/// frame seconds anywhere from 0.008 to 0.038 (each frame drawn between them), the animation's
/// locals written over every frame between the write and the world pass, 4 s, standing and
/// walking: at most one catching step, no fall, the hips lowered under 0.05 m, the floor under 3
/// body weights; the outcome said once, after the steps settle.
#[test]
fn np_a_light_hit_never_walks_into_a_fall() {
    let mut failed = Vec::new();
    for walking in [false, true] {
        let r = walk_run_with(&Walk {
            speed: 0.32,
            body: "Bip01 Spine2",
            dt: (0.008, 0.038),
            on: true,
            seconds: 4.0,
            under: 3.0,
            walking,
            writer_every: 1,
            target: None,
        });
        eprintln!(
            "RESULT np 0.32 m/s, dt 0.008-0.038, the writer every frame, walking {walking}: {} catching steps, fell {}, hips lowered at most {:.3} m, floor at most {:.2} body weights, drawn off {:.3} deg / {:.3} u, outcomes {:?}",
            r.steps, r.fell, r.lower_most, r.floor_most, r.drawn_off.0, r.drawn_off.1, r.outcomes
        );
        if r.steps > 1 {
            failed.push(format!("walking {walking}: {} steps", r.steps));
        }
        if r.fell {
            failed.push(format!("walking {walking}: fell"));
        }
        if r.lower_most >= 0.05 {
            failed.push(format!("walking {walking}: hips {:.3} m", r.lower_most));
        }
        if r.floor_most >= 3.0 {
            failed.push(format!("walking {walking}: floor {:.2}", r.floor_most));
        }
        if r.outcomes.len() != 1 {
            failed.push(format!("walking {walking}: outcomes {:?}", r.outcomes));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A run on a slope ([`slope_run`]).
struct Slope {
    /// The hits: seconds from the start, m/s, the struck body.
    hits: Vec<(f32, f32, &'static str)>,
    /// The slope (degrees, rising along +y, the way the actor walks) and the collision floor's
    /// depth under the reference (units: not at the animated feet).
    slope: f32,
    below: f32,
    /// The walk (m/s, along +y) and whether the game's mover moves it (the fake `ActorMover`
    /// update setting the character controller's velocity, through
    /// `crate::knockdown::mover_update`) rather than the test.
    walk: f32,
    mover: bool,
    seconds: f32,
}

/// What a [`slope_run`] found.
#[derive(Debug)]
struct SlopeRun {
    claimed: bool,
    /// Downed: the most frames in a row some body lay more than 1 unit under its own ground
    /// (the slope under each capsule's end), and the deepest (units).
    under_frames: u32,
    under_most: f32,
    /// When it was first downed (s), the reference's fastest second while downed (units a
    /// second, from half a second after it went down), when it stood again or gave up (s after
    /// it went down).
    downed_at: Option<f32>,
    ref_speed: f32,
    /// The game's own movement of the reference while downed (its mover and controller; units a
    /// second over the time downed).
    game_speed: f32,
    stood_after: Option<f32>,
    gave_up_after: Option<f32>,
    /// Each hit's catching steps (until the next hit or the end).
    steps: Vec<u32>,
    fell: bool,
    /// The most the floor carried in a frame (body weights) and the lines said.
    floor_most: f32,
    notes: Vec<String>,
}

/// The game's own frame on FNV's skeleton (`Fixture::game_update`, the blend collision objects,
/// downed kept) on a slope of `slope` degrees, the collision floor `below` units under the
/// reference, which the game's controller seats on it; the animation walking up it at `walk`
/// m/s, each foot lifted 5 units in turn; the writer of the animation's locals every frame, the
/// game's frame seconds anywhere from 0.008 to 0.038.
fn slope_run(cfg: &Slope) -> SlopeRun {
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
    let rise = cfg.slope.to_radians().tan();
    let mut f = Fixture::new(
        nodes,
        Ground {
            collision: Some(at[2] - cfg.below),
            navmesh: at[2] - cfg.below - 8.0,
        },
    );
    rig_fixture::SLOPE.with(|s| s.set(Some([at[0], at[1], 0.0, rise])));
    let ground = |p: V3| rig_fixture::floor_at(p).unwrap_or(at[2] - cfg.below);
    f.game_update();
    f.make_character();
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    set_keep_down(true);
    let was = crate::gunplay::tuning();
    crate::gunplay::set_tuning(crate::gunplay::Tuning {
        new_physics: true,
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
    // The actor's `ActorMover` (its actor at +0x28).
    const MOVER: u32 = 0x0300_2000;
    f.e.mem.map(MOVER, 0x1000);
    f.e.mem
        .set_u32(MOVER + crate::knockdown::MOVER_ACTOR, ACTOR);
    let velocity_at = rig_fixture::CONTROLLER + 0x700;
    let body_nodes: Vec<usize> = model()
        .bodies
        .iter()
        .map(|b| f.nodes.iter().position(|n| n.name == b.node).unwrap())
        .collect();
    let root = f.at[0];
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
    let pose_leg = |f: &mut Fixture, side: usize, a: f32| {
        let [thigh, calf] = legs[side];
        f.nodes[thigh].r = turn_z(&rest[thigh], a);
        f.nodes[calf].r = turn_z(&rest[calf], -2.0 * a);
    };
    rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, [0.0; 3]);
    f.animate();
    let low0 = foot_low(&f, 0);
    let mut swing = 0.0f32;
    for i in 1..=80 {
        let a = i as f32 * 0.5;
        pose_leg(&mut f, 0, a);
        f.animate();
        if foot_low(&f, 0) - low0 >= WALK_LIFT {
            swing = a;
            break;
        }
    }
    pose_leg(&mut f, 0, 0.0);
    f.animate();
    let anim0: Vec<(M3, V3)> = body_nodes.iter().map(|&i| f.world_of(f.at[i])).collect();
    let low = ragdoll_core::np_body::lowest_point(model(), &model().states_from_nodes(&anim0)) * k;
    // The animated feet 1 unit under the reference (the clip's take: -0.7).
    let drop = [0.0, 0.0, -low - 1.0];
    rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, at);
    room_events::set_on(true);
    let _ = room_events::take();
    let mut run = SlopeRun {
        claimed: false,
        under_frames: 0,
        under_most: 0.0,
        downed_at: None,
        ref_speed: 0.0,
        game_speed: 0.0,
        stood_after: None,
        gave_up_after: None,
        steps: Vec::new(),
        fell: false,
        floor_most: 0.0,
        notes: Vec::new(),
    };
    let weight = model().mass * 9.81;
    let mut seed = 0x2468_1357u32;
    let mut t = 0.0f32;
    let mut frame = 0u32;
    let mut hits = cfg.hits.clone();
    let mut steps_at: Option<u32> = None;
    let mut under_run = 0u32;
    let mut down_refs: Vec<(f32, V3)> = Vec::new();
    let mut was_down = false;
    let (mut game_moved, mut down_time) = (0.0f32, 0.0f32);
    let cos = cfg.slope.to_radians().cos();
    while t < cfg.seconds {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let dt = 0.008 + 0.030 * ((seed >> 8) as f32 / (1u32 << 24) as f32);
        t += dt;
        f.e.mem.set_f32(0x011d_ea30, dt);
        AI_BUSY.store(false, Ordering::Relaxed);
        // The game's movement: its mover (held by New Physics while downed) or the AI's walk,
        // then its controller seating the reference on the slope.
        let mut reference = f.reference();
        // The animation walks only while the game moves the actor (a held mover's speed is 0:
        // the game plays no walk).
        let mut walking = 0.0;
        if cfg.mover {
            let want = [0.0, cfg.walk * k, 0.0];
            crate::knockdown::mover_update(&mut f.e, MOVER, |e| {
                rig_fixture::write3(e, velocity_at, want);
                rig_fixture::write3(e, MOVER + crate::knockdown::MOVER_DELTA, scale(want, dt));
            });
            let v = rig_fixture::read3(&f.e, velocity_at);
            reference = add(reference, scale(v, dt));
            if is_down(ACTOR) {
                game_moved += length(v) * dt;
                down_time += dt;
            }
            if length(v) > 0.0 {
                walking = 1.0;
            }
        } else if !owns(ACTOR) && cfg.walk > 0.0 {
            // The AI's walk (New Physics owns the reference while it has the actor).
            reference = add(reference, [0.0, cfg.walk * k * dt, 0.0]);
            walking = 1.0;
        }
        reference[2] = ground(reference) + cfg.below;
        rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, reference);
        main_thread(&mut f.e);
        let reference = f.reference();
        let phase = (std::f32::consts::TAU * t / WALK_PERIOD).sin();
        pose_leg(&mut f, 0, walking * swing * phase.max(0.0).sqrt());
        pose_leg(&mut f, 1, walking * swing * (-phase).max(0.0).sqrt());
        let root_at = add(reference, drop);
        rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, root_at);
        f.animate();
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
        if hits.first().is_some_and(|h| t >= h.0) {
            let (_, speed, body) = hits.remove(0);
            run.claimed |= claim(&mut f.e, ACTOR);
            let now = state(ACTOR).map_or(0, |s| s.1);
            if let Some(was) = steps_at.replace(now) {
                run.steps.push(now - was);
            }
            let struck = model().body(body).unwrap();
            hit(
                &mut f.e,
                ACTOR,
                speed * model().mass,
                add(anim[struck].1, [0.0, 6.0, 2.0]),
                [0.0, -1.0, 0.0],
            );
        }
        AI_BUSY.store(true, Ordering::Relaxed);
        crate::active_ragdoll::apply(&mut f.e);
        // The writer: the animation's locals set again every frame, the worlds from them.
        for (i, &n) in f.at.iter().enumerate().skip(1) {
            rig_fixture::write9(&mut f.e, n + rig_fixture::LOCAL_ROTATION, &locals[i].0);
            rig_fixture::write3(&mut f.e, n + rig_fixture::LOCAL_TRANSLATION, locals[i].1);
        }
        f.update();
        crate::active_ragdoll::read_drawn(&mut f.e);
        let down = is_down(ACTOR);
        if owns(ACTOR) {
            if let Some(n) = floor_force(ACTOR) {
                run.floor_most = run.floor_most.max(n / weight);
            }
            if let Some((phase, _, _)) = state(ACTOR) {
                run.fell |= phase == ragdoll_core::np_balance::Phase::Falling;
            }
            run.fell |= down;
        }
        if down {
            run.downed_at.get_or_insert(t);
            down_refs.push((t, f.reference()));
            // Each capsule's ends against the slope under them.
            let mut under = 0.0f32;
            if let Some((p, _)) = drawn(ACTOR) {
                for (b, def) in model().bodies.iter().enumerate() {
                    let (r, x) = p[b];
                    for e in def.ends {
                        let q = add(x, apply_m(&r, scale(e, k)));
                        let d = (q[2] - ground(q)) * cos - def.radius * k;
                        under = under.max(-d);
                    }
                }
            }
            run.under_most = run.under_most.max(under);
            under_run = if under > 1.0 { under_run + 1 } else { 0 };
            run.under_frames = run.under_frames.max(under_run);
        } else if was_down && owns(ACTOR) && run.stood_after.is_none() {
            run.stood_after = run.downed_at.map(|d| t - d);
        }
        if run.gave_up_after.is_none()
            && run.downed_at.is_some()
            && (f.e.mem.u8(rig_fixture::PROCESS_AT + 0x13c) != 0 || !falls().is_empty())
        {
            run.gave_up_after = run.downed_at.map(|d| t - d);
        }
        was_down |= down;
        if std::env::var_os("NP_TRACE").is_some() {
            let pel = drawn(ACTOR).map(|p| p.0[model().root].1);
            eprintln!(
                "TRACE t {t:.3} dt {dt:.3} floor {:.2} state {:?} ref {:?} pelvis {:?} down {down} label {:?} v {:?} {}",
                floor_force(ACTOR).unwrap_or(0.0) / weight,
                state(ACTOR).map(|s| (s.0, s.1)),
                f.reference().map(|x| x.round()),
                pel.map(|p| p.map(|x| x.round())),
                label_of(ACTOR),
                rig_fixture::read3(&f.e, velocity_at),
                sample_field(ACTOR)
            );
        }
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
        for l in room_events::take() {
            for key in [
                "[np outcome]",
                "[np fall]",
                "[np floor load]",
                "[np choice]",
            ] {
                if let Some(i) = l.find(key) {
                    run.notes
                        .push(l[i..].trim_end_matches(['"', '}']).to_string());
                }
            }
        }
        frame += 1;
    }
    let _ = frame;
    if let Some(was) = steps_at {
        run.steps
            .push(state(ACTOR).map_or(0, |s| s.1).saturating_sub(was));
    }
    // The reference's fastest second while downed, from half a second after it went down.
    if let Some(d) = run.downed_at {
        let pts: Vec<&(f32, V3)> = down_refs.iter().filter(|p| p.0 >= d + 0.5).collect();
        for (i, a) in pts.iter().enumerate() {
            if let Some(b) = pts[i..].iter().find(|b| b.0 >= a.0 + 1.0) {
                let s = sub(b.1, a.1);
                run.ref_speed = run.ref_speed.max(length([s[0], s[1], 0.0]) / (b.0 - a.0));
            }
        }
    }
    if down_time > 0.0 {
        run.game_speed = game_moved / down_time;
    }
    room_events::set_on(false);
    AI_BUSY.store(false, Ordering::Relaxed);
    rig_fixture::SLOPE.with(|s| s.set(None));
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    crate::gunplay::set_tuning(was);
    run
}

/// Patrick's 23:41 clip (Goodsprings' slope): walking up a 20 degree slope at 0.45 m/s, the
/// collision floor 7 units under the reference, a 4.3 m/s hit downs it; downed, no body lies more
/// than 1 unit under its own ground for more than 2 frames running (a flat floor that followed
/// the reference put shins and knees into the slope).
#[test]
fn np_slope_downed_lies_on_the_slope() {
    let r = slope_run(&Slope {
        hits: vec![(0.4, 4.3, "Bip01 Spine2")],
        slope: 20.0,
        below: 7.0,
        walk: 0.45,
        mover: false,
        seconds: 5.0,
    });
    eprintln!(
        "RESULT np slope downed: downed at {:?} s, a body more than 1 u under its ground for at most {} frames running, the deepest {:.1} u, floor at most {:.1} body weights",
        r.downed_at, r.under_frames, r.under_most, r.floor_most
    );
    assert!(r.claimed && r.downed_at.is_some(), "{r:?}");
    assert!(
        r.under_frames <= 2,
        "{} frames, {:.1} u",
        r.under_frames,
        r.under_most
    );
}

/// The same, the game's mover and controller moving the reference at 30 units a second (the
/// clip's glide) while it is downed: the game moves the reference under 2 units a second while it
/// is down (New Physics holds the game's movement before it applies; the reference follows the
/// pelvis, which the fall, the rises and a collapse move), and it stands within 4 s of going
/// down or gives up into the game's ragdoll (a rise lasts at most 2 s).
#[test]
fn np_slope_downed_holds_the_games_movement() {
    let r = slope_run(&Slope {
        hits: vec![(0.4, 4.3, "Bip01 Spine2")],
        slope: 20.0,
        below: 7.0,
        walk: 30.0 / 70.0,
        mover: true,
        seconds: 12.0,
    });
    eprintln!(
        "RESULT np slope held: downed at {:?} s, the game moved the reference {:.2} u/s while down, the reference (following the pelvis) at most {:.1} u/s over a second, stood after {:?} s, gave up after {:?} s; {:?}",
        r.downed_at, r.game_speed, r.ref_speed, r.stood_after, r.gave_up_after,
        r.notes.iter().filter(|n| n.contains("[np choice]") || n.contains("[np fall]")).collect::<Vec<_>>()
    );
    assert!(r.downed_at.is_some(), "{r:?}");
    assert!(r.game_speed < 2.0, "{:.2} u/s", r.game_speed);
    assert!(
        r.stood_after.is_some_and(|s| s <= 4.0) || r.gave_up_after.is_some(),
        "stood {:?}, gave up {:?}",
        r.stood_after,
        r.gave_up_after
    );
}

/// A walking AI (0.45 m/s up the slope) and each hit's steps its own: three 0.69 m/s hits 1.5 s
/// apart, no fall and at most 3 catching steps each; a 0.28 m/s hit at most 1; a 4.3 m/s hit
/// falls.
#[test]
fn np_slope_steps_are_each_hits_own() {
    let mut failed = Vec::new();
    let r = slope_run(&Slope {
        hits: vec![
            (0.4, 0.69, "Bip01 Spine2"),
            (1.9, 0.69, "Bip01 Spine2"),
            (3.4, 0.69, "Bip01 Spine2"),
        ],
        slope: 20.0,
        below: 7.0,
        walk: 0.45,
        mover: false,
        seconds: 5.5,
    });
    eprintln!(
        "RESULT np slope three 0.69 m/s hits: steps {:?}, fell {}, floor at most {:.2} body weights; {:?}",
        r.steps, r.fell, r.floor_most,
        r.notes.iter().filter(|n| n.contains("[np fall]") || n.contains("[np outcome]")).collect::<Vec<_>>()
    );
    if r.fell || r.steps.iter().any(|&s| s > 3) || r.steps.len() != 3 {
        failed.push(format!("0.69: steps {:?}, fell {}", r.steps, r.fell));
    }
    let r = slope_run(&Slope {
        hits: vec![(0.4, 0.28, "Bip01 Spine2")],
        slope: 20.0,
        below: 7.0,
        walk: 0.45,
        mover: false,
        seconds: 3.5,
    });
    eprintln!(
        "RESULT np slope 0.28 m/s: steps {:?}, fell {}, floor at most {:.2} body weights",
        r.steps, r.fell, r.floor_most
    );
    if r.fell || r.steps.iter().any(|&s| s > 1) {
        failed.push(format!("0.28: steps {:?}, fell {}", r.steps, r.fell));
    }
    let r = slope_run(&Slope {
        hits: vec![(0.4, 4.3, "Bip01 Spine2")],
        slope: 20.0,
        below: 7.0,
        walk: 0.45,
        mover: false,
        seconds: 2.0,
    });
    eprintln!("RESULT np slope 4.3 m/s: fell {}", r.fell);
    if !r.fell {
        failed.push("4.3: no fall".to_string());
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The floor under 3 body weights for hits up to 0.7 m/s on the slope (the clip's 19 to 32 on
/// 0.28 to 0.69 m/s hits), walking up it at 0.45 m/s.
#[test]
fn np_slope_light_hits_load_the_floor_under_three_weights() {
    let mut failed = Vec::new();
    for speed in [0.28f32, 0.5, 0.69] {
        let r = slope_run(&Slope {
            hits: vec![(0.4, speed, "Bip01 Spine2")],
            slope: 20.0,
            below: 7.0,
            walk: 0.45,
            mover: false,
            seconds: 3.0,
        });
        eprintln!(
            "RESULT np slope {speed} m/s: floor at most {:.2} body weights; {:?}",
            r.floor_most,
            r.notes
                .iter()
                .filter(|n| n.contains("[np floor load]"))
                .collect::<Vec<_>>()
        );
        if r.floor_most >= 3.0 {
            failed.push(format!("{speed}: {:.2} body weights", r.floor_most));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A run of the whole cycle ([`cycle_run`]).
struct Cycle {
    /// The slope (degrees, rising along +y) and the collision floor's depth under the reference.
    slope: f32,
    below: f32,
    /// The script: a 4.8 m/s hit; once it stands again, three 0.32 m/s hits 1.5 s apart; then
    /// (when `give_up`) a 4.8 m/s hit and, downed, its rises given up into the game's ragdoll and
    /// the old get-up.
    give_up: bool,
    seconds: f32,
    /// The upper arms' turn off the skeleton's pose (times 50 degrees).
    arms: f32,
    /// The world's boxes the casts meet (each its least and most corner, units, about the
    /// reference at the origin with the collision floor `below` under it): walls, crates, slabs.
    boxes: Vec<(V3, V3)>,
    /// The downing hits' speed (m/s; 4.8 when 0) and their way (flat; -y when zero).
    heavy: f32,
    knock: V3,
    /// The light hits' speed (m/s; 0.32 when 0), each in this many pellets in one frame (1 when
    /// 0) at points a few units apart, summing to it.
    light: f32,
    pellets: u32,
    /// It holds a weapon drawn (the right hand's).
    armed: bool,
}

impl Default for Cycle {
    fn default() -> Self {
        Cycle {
            slope: 0.0,
            below: 1.0,
            give_up: false,
            seconds: 18.0,
            arms: 0.0,
            boxes: Vec::new(),
            heavy: 0.0,
            knock: [0.0; 3],
            light: 0.0,
            pellets: 0,
            armed: false,
        }
    }
}

/// What a [`cycle_run`] found.
#[derive(Debug, Default)]
struct CycleRun {
    downed: u32,
    stood: u32,
    /// Downed: the most a body's drawn rotation turned in a frame beyond what its spin carried it
    /// (degrees), the frames over 20 (a snap).
    turn_most: f32,
    snaps: u32,
    /// Downed: the reference's fastest second (units a second), the game's own movement of it.
    ref_speed: f32,
    game_speed: f32,
    /// The light hits: how many, any fall from them, the most catching steps one took.
    light_hits: u32,
    light_fell: bool,
    light_steps: u32,
    /// Catching steps begun 1.5 s or more after the last hit, standing (none of a hit's).
    stray_steps: u32,
    /// Standing (not downed or falling): the most the floor carried (body weights).
    floor_most: f32,
    /// Every frame New Physics had it: drawn at the world pass against simulated.
    drawn_off: (f32, f32),
    /// The hand-over: whether it came, the pose handed outside the skeleton's limits (degrees),
    /// the clamp's frames after it, the get-up's frames upside down and lowest node over the
    /// collision floor (units).
    handed: bool,
    handed_out: f32,
    clamps: u32,
    get_up: Option<(u32, f32)>,
    get_up_any_node: f32,
    jumps: u32,
    notes: Vec<String>,
    /// Every frame New Physics had it: the deepest any body's capsule lay inside a box (units),
    /// which body and when (s after the last hit, downed).
    inside_most: f32,
    inside_at: String,
    /// The `[np wall]` lines and the most a wall carried (N, the outcomes').
    wall_logged: u32,
    wall_most: f32,
    /// The fastest body (m/s, drawn) past 0.3 s after the last hit, standing or downed past a
    /// second (not the fall the hit causes).
    fast_most: f32,
    /// Seconds from going down to standing again, the first time; and the most from first
    /// reading upright (labelled standing) to standing again.
    first_stood: Option<f32>,
    upright_still: f32,
    /// The light hits' catches (length, how far the landing left the capture point outside; m).
    catches: Vec<(f32, f32)>,
    /// Downed: the most the hands carried of the body's weight while planted before the legs
    /// push (its share), the most a landed hand slid (units), the times a landed hand's load went
    /// and came back while planted (a bounce), the most one rose off its spot while planted
    /// (units, its lowest point), the frames a weapon hand was planted, the frames any was.
    hand_share: f32,
    hand_slip: f32,
    hand_bounce: u32,
    hand_lift: f32,
    weapon_planted: u32,
    planted_frames: u32,
    /// The reference furthest along the knock (the most it moved to the body), the most any
    /// reference position lay inside a box (units, the actor's capsule's radius taken in) and
    /// the moves swept (`[np move]`).
    ref_end: V3,
    ref_inside: f32,
    moves: u32,
}

/// The whole cycle on FNV's skeleton through the game's own path: the game's update
/// (`Fixture::game_update`, the blend collision objects), downed kept, the game's mover walking
/// the AI at 0.45 m/s (held while New Physics has it), the game's frame seconds anywhere from
/// 0.008 to 0.038, and a fake animation thread: each frame the animation's update (its locals,
/// the worlds, New Physics' snapshot after it) runs at a random point of the frame (before the
/// main thread's update, before the draw's set-up, between the set-up and the world pass, or
/// after the draw), and about a third of the frames it is caught half-way during the set-up
/// (half the bones' locals at the bind pose's, no snapshot), left so until its next update.
fn cycle_run(cfg: &Cycle) -> CycleRun {
    use std::sync::atomic::{AtomicBool, Ordering};
    static AI_BUSY: AtomicBool = AtomicBool::new(false);
    static TORN: Mutex<Vec<(u32, M3, V3)>> = Mutex::new(Vec::new());
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut nodes = rig_fixture::fnv_skeleton();
    let bind: Vec<(M3, V3)> = nodes.iter().map(|n| (n.r, n.t)).collect();
    let turn_z = |r: &M3, deg: f32| {
        let (s, c) = deg.to_radians().sin_cos();
        let rz: M3 = [c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0];
        ragdoll_core::flinch::mul(r, &rz)
    };
    for (name, deg) in [
        ("Bip01 L UpperArm", 50.0f32 * cfg.arms),
        ("Bip01 R UpperArm", 50.0 * cfg.arms),
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
    let at: V3 = [0.0, 0.0, 0.0];
    let rise = cfg.slope.to_radians().tan();
    let mut f = Fixture::new(
        nodes,
        Ground {
            collision: Some(at[2] - cfg.below),
            navmesh: at[2] - cfg.below - 8.0,
        },
    );
    if cfg.slope != 0.0 {
        rig_fixture::SLOPE.with(|s| s.set(Some([at[0], at[1], 0.0, rise])));
    }
    let ground = |p: V3| rig_fixture::floor_at(p).unwrap_or(at[2] - cfg.below);
    if !cfg.boxes.is_empty() {
        rig_fixture::WORLD.with(|w| *w.borrow_mut() = Some(cfg.boxes.clone()));
    }
    rig_fixture::ARMED.with(|a| a.set(cfg.armed));
    let heavy = if cfg.heavy > 0.0 { cfg.heavy } else { 4.8 };
    let knock = if length(cfg.knock) > 0.0 {
        cfg.knock
    } else {
        [0.0, -1.0, 0.0]
    };
    let light = if cfg.light > 0.0 { cfg.light } else { 0.32 };
    let pellets = cfg.pellets.max(1);
    let mut hand_spots: [Option<V3>; 2] = [None, None];
    let mut hand_off = [false; 2];
    let mut upright_at: Option<f32> = None;
    let mut last_bodies: Option<Vec<V3>> = None;
    f.game_update();
    f.make_character();
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    set_keep_down(true);
    let was = crate::gunplay::tuning();
    crate::gunplay::set_tuning(crate::gunplay::Tuning {
        new_physics: true,
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
    // The actor's 3D (`Actor::Get3D`, which the draw's set-up asks first): the animation thread
    // caught half-way writes its torn locals then.
    let root = f.at[0];
    f.e.register_double(0x0043_fcd0, move |e, a| {
        for (n, r, t) in std::mem::take(&mut *TORN.lock().unwrap_or_else(|p| p.into_inner())) {
            rig_fixture::write9(e, n + rig_fixture::LOCAL_ROTATION, &r);
            rig_fixture::write3(e, n + rig_fixture::LOCAL_TRANSLATION, t);
        }
        if a[0] == ACTOR { root } else { 0 }.into_ret()
    });
    const MOVER: u32 = 0x0300_2000;
    f.e.mem.map(MOVER, 0x1000);
    f.e.mem
        .set_u32(MOVER + crate::knockdown::MOVER_ACTOR, ACTOR);
    let velocity_at = rig_fixture::CONTROLLER + 0x700;
    let body_nodes: Vec<usize> = model()
        .bodies
        .iter()
        .map(|b| f.nodes.iter().position(|n| n.name == b.node).unwrap())
        .collect();
    let k = ragdoll_core::np_body::UNITS;
    f.animate();
    let anim0: Vec<(M3, V3)> = body_nodes.iter().map(|&i| f.world_of(f.at[i])).collect();
    let low = ragdoll_core::np_body::lowest_point(model(), &model().states_from_nodes(&anim0)) * k;
    let drop = [0.0, 0.0, -low - 1.0];
    rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, at);
    room_events::set_on(true);
    let _ = room_events::take();
    let clamps0 = crate::knockdown::CLAMPS.load(Ordering::Relaxed);
    let weight = model().mass * 9.81;
    let mut run = CycleRun::default();
    let mut seed = 0x1357_2468u32;
    let mut rnd = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    let mut t = 0.0f32;
    // The script's state: the next hit's time and speed, the light hits left, when it stood.
    let mut next_hit: Option<(f32, f32)> = Some((0.4, heavy));
    let mut lights_left = 3u32;
    let mut stage = 0u32;
    let mut last_hit = 0.0f32;
    let mut last_steps = 0u32;
    let mut hit_steps_from = 0u32;
    let mut catches_from = 0usize;
    let mut was_down = false;
    let mut downed_at = 0.0f32;
    let mut down_refs: Vec<(f32, V3)> = Vec::new();
    let (mut game_moved, mut down_time) = (0.0f32, 0.0f32);
    let mut last_drawn: Option<Pose> = None;
    let mut last_spin: Vec<V3> = Vec::new();
    let mut ragdoll_pose: Vec<(M3, V3)> = Vec::new();
    let mut gave_up = false;
    while t < cfg.seconds {
        let dt = 0.008 + 0.030 * rnd();
        t += dt;
        f.e.mem.set_f32(0x011d_ea30, dt);
        AI_BUSY.store(false, Ordering::Relaxed);
        let slot = (rnd() * 4.0) as u32;
        let torn = rnd() < 0.35;
        // The game's movement: its mover (held by New Physics while it has the actor), its
        // controller seating the reference on the ground.
        let want = [0.0, 0.45 * k, 0.0];
        crate::knockdown::mover_update(&mut f.e, MOVER, |e| {
            rig_fixture::write3(e, velocity_at, want);
            rig_fixture::write3(e, MOVER + crate::knockdown::MOVER_DELTA, scale(want, dt));
        });
        let v = rig_fixture::read3(&f.e, velocity_at);
        let mut reference = add(f.reference(), scale(v, dt));
        if is_down(ACTOR) {
            game_moved += length(v) * dt;
            down_time += dt;
        }
        reference[2] = ground(reference) + cfg.below;
        rig_fixture::write3(&mut f.e, ACTOR + rig_fixture::REFR_POSITION, reference);
        let walking = length(v) > 0.0;
        // The animation's update (its locals for this frame, the worlds, the snapshot).
        let animate = |f: &mut Fixture| {
            let reference = f.reference();
            let root_at = add(reference, drop);
            let phase = (std::f32::consts::TAU * t / WALK_PERIOD).sin();
            let swing = if walking { 12.0 * phase } else { 0.0 };
            for (name, a) in [("Bip01 L Thigh", swing), ("Bip01 R Thigh", -swing)] {
                let i = f.nodes.iter().position(|n| n.name == name).unwrap();
                f.nodes[i].r = turn_z(&bind[i].0, a);
            }
            rig_fixture::write3(&mut f.e, f.at[0] + rig_fixture::LOCAL_TRANSLATION, root_at);
            f.animate();
        };
        if slot == 0 {
            animate(&mut f);
        }
        main_thread(&mut f.e);
        crate::knockdown::frame(&mut f.e);
        if slot == 1 {
            animate(&mut f);
        }
        // The script.
        let (phase_now, steps_now, _) =
            state(ACTOR).unwrap_or((ragdoll_core::np_balance::Phase::Standing, 0, [0.0; 3]));
        if let Some((when, speed)) = next_hit {
            if t >= when {
                next_hit = None;
                run.light_steps = run
                    .light_steps
                    .max(steps_now.saturating_sub(hit_steps_from));
                if speed < 1.0 {
                    if let Some(p) = probe(ACTOR) {
                        run.catches.extend(p.catches.iter().skip(catches_from));
                    }
                }
                claim(&mut f.e, ACTOR);
                let spine = model().body("Bip01 Spine2").unwrap();
                let chest = f.world_of(f.at[body_nodes[spine]]).1;
                let n = if speed < 1.0 { pellets } else { 1 };
                for i in 0..n {
                    // A burst's pellets a few units apart over the chest.
                    let a = i as f32 * 2.4;
                    let spread = if n > 1 {
                        [4.0 * a.cos(), 0.0, 4.0 * a.sin()]
                    } else {
                        [0.0; 3]
                    };
                    hit(
                        &mut f.e,
                        ACTOR,
                        speed / n as f32 * model().mass,
                        add(add(chest, [0.0, -knock[1] * 6.0, 2.0]), spread),
                        knock,
                    );
                }
                catches_from = probe(ACTOR).map_or(0, |p| p.catches.len());
                last_hit = t;
                hit_steps_from = state(ACTOR).map_or(0, |s| s.1);
                if speed < 1.0 {
                    run.light_hits += 1;
                }
            }
        }
        if torn {
            let mut tear = TORN.lock().unwrap_or_else(|p| p.into_inner());
            for (i, &n) in f.at.iter().enumerate().skip(1) {
                if i % 2 == 0 {
                    tear.push((n, bind[i].0, bind[i].1));
                }
            }
        }
        AI_BUSY.store(true, Ordering::Relaxed);
        crate::active_ragdoll::apply(&mut f.e);
        TORN.lock().unwrap_or_else(|p| p.into_inner()).clear();
        if slot == 2 {
            animate(&mut f);
        }
        // The world pass.
        crate::active_ragdoll::read_drawn(&mut f.e);
        let down = is_down(ACTOR);
        if owns(ACTOR) {
            if let Some((p, _)) = drawn(ACTOR) {
                for (b, &i) in body_nodes.iter().enumerate() {
                    let w = f.world_of(f.at[i]);
                    run.drawn_off.0 = run
                        .drawn_off
                        .0
                        .max(ragdoll_core::np_skeleton::angle_between(&w.0, &p[b].0).to_degrees());
                    run.drawn_off.1 = run.drawn_off.1.max(length(sub(w.1, p[b].1)));
                }
                let spin_now = spins(ACTOR).unwrap_or_default();
                // Downed, past the hit's own first 0.1 s (its impulse turns the body within the
                // frame faster than the frame's end spin says).
                if down && t - last_hit > 0.1 {
                    if let Some(was) = last_drawn.as_ref() {
                        // A body's turn in the frame beyond what its spin (before and after)
                        // carried it: a snap, not the fall's own motion.
                        let turn = p
                            .iter()
                            .zip(was)
                            .enumerate()
                            .map(|(b, (a, w))| {
                                let spin = spin_now
                                    .get(b)
                                    .map_or(0.0, |s| length(*s))
                                    .max(last_spin.get(b).map_or(0.0, |s| length(*s)));
                                (
                                    ragdoll_core::np_skeleton::angle_between(&a.0, &w.0)
                                        .to_degrees()
                                        - (spin * dt).to_degrees(),
                                    b,
                                )
                            })
                            .fold((0.0f32, 0usize), |m, x| if x.0 > m.0 { x } else { m });
                        let (turn, turn_body) = turn;
                        run.turn_most = run.turn_most.max(turn);
                        run.snaps += u32::from(turn > 20.0);
                        if std::env::var_os("NP_TRACE").is_some() && turn > 15.0 {
                            eprintln!(
                                "SNAP t {t:.3} dt {dt:.3} turn {turn:.1} {} since down {:.2} label {:?} {}",
                                model().bodies[turn_body].node,
                                t - downed_at,
                                label_of(ACTOR),
                                sample_field(ACTOR)
                            );
                        }
                    }
                }
                last_drawn = Some(p);
                last_spin = spin_now;
            }
            ragdoll_pose = f.worlds();
            // The bodies against the world's boxes, and their speed.
            if let Some((p, _)) = drawn(ACTOR) {
                let k = ragdoll_core::np_body::UNITS;
                let mut centres = Vec::new();
                for (b, def) in model().bodies.iter().enumerate() {
                    let (r, x) = p[b];
                    let ends: Vec<V3> = def
                        .ends
                        .iter()
                        .map(|e| add(x, apply_m(&r, scale(*e, k))))
                        .collect();
                    let mid = scale(add(ends[0], ends[ends.len() - 1]), 0.5);
                    centres.push(mid);
                    let rad = def.radius * k;
                    for e in ends.iter().copied().chain([mid]) {
                        for (lo, hi) in &cfg.boxes {
                            let depth = (0..3)
                                .map(|i| (e[i] - (lo[i] - rad)).min(hi[i] + rad - e[i]))
                                .fold(f32::INFINITY, f32::min);
                            if depth > run.inside_most {
                                run.inside_most = depth;
                                run.inside_at = format!(
                                    "{} {:.2} s after the hit, downed {down}, {:?}, at {:?}",
                                    def.node,
                                    t - last_hit,
                                    label_of(ACTOR),
                                    e.map(|x| x.round())
                                );
                            }
                        }
                    }
                }
                if let Some(was) = &last_bodies {
                    // Past the hit's own (its push, the fall it causes): standing, or downed
                    // past a second (settled, rising).
                    if t - last_hit > 0.3 && (!down || t - downed_at > 1.0) {
                        for (c, w) in centres.iter().zip(was) {
                            run.fast_most = run.fast_most.max(length(sub(*c, *w)) / dt / k);
                        }
                    }
                }
                last_bodies = Some(centres);
            }
            if let Some(pr) = probe(ACTOR) {
                let weight = model().mass * ragdoll_core::np_body::GRAVITY;
                if pr.hands.iter().any(|h| h.is_some()) {
                    run.planted_frames += 1;
                    if !pr.lifting {
                        run.hand_share = run
                            .hand_share
                            .max((pr.hand_load[0] + pr.hand_load[1]) / weight);
                    }
                }
                for side in 0..2 {
                    if pr.hands[side].is_some() && Some(side) == pr.weapon_side {
                        run.weapon_planted += 1;
                    }
                    // A bounce: a landed hand's load gone and back while it stays planted.
                    match (pr.hands[side], pr.landed[side]) {
                        (Some(_), true) if pr.hand_load[side] <= 1.0 => hand_off[side] = true,
                        (Some(_), true) if hand_off[side] => {
                            run.hand_bounce += 1;
                            hand_off[side] = false;
                        }
                        _ => {}
                    }
                    if pr.hands[side].is_none() {
                        hand_off[side] = false;
                    }
                    match (pr.hands[side], pr.landed[side]) {
                        (Some(_), true) => {
                            let at = pr.hand_at[side];
                            let low = pr.hand_low[side];
                            let spot = *hand_spots[side].get_or_insert([at[0], at[1], low]);
                            run.hand_slip =
                                run.hand_slip
                                    .max(length([at[0] - spot[0], at[1] - spot[1], 0.0]));
                            run.hand_lift = run.hand_lift.max(low - spot[2]);
                        }
                        _ => hand_spots[side] = None,
                    }
                }
            }
            if !down && phase_now != ragdoll_core::np_balance::Phase::Falling {
                if let Some(n) = floor_force(ACTOR) {
                    run.floor_most = run.floor_most.max(n / weight);
                }
            }
            if steps_now > last_steps && t - last_hit >= 1.5 && !down {
                run.stray_steps += steps_now - last_steps;
            }
            last_steps = steps_now;
        } else {
            last_drawn = None;
            last_bodies = None;
        }
        // The reference against the boxes (the actor's capsule's radius in).
        {
            let rf = f.reference();
            if ragdoll_core::flinch::dot(rf, knock) > ragdoll_core::flinch::dot(run.ref_end, knock)
            {
                run.ref_end = rf;
            }
            let radius = crate::knockdown::capsule(&mut f.e, ACTOR);
            for (lo, hi) in &cfg.boxes {
                for h in [12.0f32, 30.0] {
                    let p = add(rf, [0.0, 0.0, h]);
                    if p[2] < lo[2] || p[2] > hi[2] {
                        continue;
                    }
                    let depth = (0..2)
                        .map(|i| (p[i] - (lo[i] - radius)).min(hi[i] + radius - p[i]))
                        .fold(f32::INFINITY, f32::min);
                    run.ref_inside = run.ref_inside.max(depth);
                }
            }
        }
        if down {
            if !was_down {
                run.downed += 1;
                downed_at = t;
                down_refs.clear();
                upright_at = None;
            }
            if upright_at.is_none() && label_of(ACTOR) == Some("standing") && t - downed_at > 0.5 {
                upright_at = Some(t);
            }
            down_refs.push((t, f.reference()));
            // Its strength gone (it cannot push itself up), its rises given up once it lies still
            // (as after three that failed).
            if stage == 2 && cfg.give_up {
                set_effort(ACTOR, 0.0);
            }
            if stage == 2
                && lights_left == 0
                && cfg.give_up
                && !gave_up
                && t > downed_at + 2.5
                && label_of(ACTOR) == Some("lying")
            {
                gave_up = true;
                give_up(ACTOR);
            }
        } else if was_down && owns(ACTOR) {
            run.stood += 1;
            if run.first_stood.is_none() {
                run.first_stood = Some(t - downed_at);
            }
            if let Some(u) = upright_at {
                run.upright_still = run.upright_still.max(t - u);
            }
            if stage == 0 {
                stage = 1;
                next_hit = Some((t + 1.0, light));
            }
        }
        if stage == 1 && next_hit.is_none() && t - last_hit >= 1.5 && !down {
            if lights_left > 0 {
                lights_left -= 1;
                if lights_left > 0 {
                    next_hit = Some((t, light));
                } else if cfg.give_up {
                    stage = 2;
                    next_hit = Some((t, heavy));
                }
            }
        }
        if stage == 1 && run.light_hits > 0 && phase_now == ragdoll_core::np_balance::Phase::Falling
        {
            run.light_fell = true;
        }
        if stage == 1 && down && run.light_hits > 0 {
            run.light_fell = true;
        }
        was_down = down;
        if slot == 3 {
            animate(&mut f);
        }
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
        for l in room_events::take() {
            for key in [
                "[np fall]",
                "[np hand-over]",
                "[np jump]",
                "[np outcome]",
                "[np wall]",
                "[np move]",
                "[np choice]",
                "[np floor load]",
            ] {
                if let Some(i) = l.find(key) {
                    run.notes
                        .push(l[i..].trim_end_matches(['"', '}']).to_string());
                }
            }
        }
        if let Some(fall) = falls().first() {
            run.handed = true;
            // The pose handed against the skeleton's own limits (the game's ragdoll's).
            let sk = ragdoll_core::np_skeleton::Skeleton::fnv();
            let world_of = |name: &str| {
                let i = f.nodes.iter().position(|n| n.name == name)?;
                fall.pose.iter().find(|p| p.0 == f.at[i]).map(|p| p.1)
            };
            for j in &sk.joints {
                if let (Some(c), Some(p)) = (world_of(&j.child), world_of(&j.parent)) {
                    let out = j.outside(j.measure(&c, &p)).to_degrees();
                    // Taking in the bind pose's own (the neck's 17 degrees out in the data).
                    let bind_out = {
                        let bw = sk.bind_worlds();
                        j.outside(j.measure(
                            &bw[sk.node(&j.child).unwrap()].r,
                            &bw[sk.node(&j.parent).unwrap()].r,
                        ))
                        .to_degrees()
                    };
                    if std::env::var_os("NP_TRACE").is_some() {
                        eprintln!("LIMIT {} {out:.1} (bind {bind_out:.1})", j.child);
                    }
                    run.handed_out = run.handed_out.max(out - bind_out);
                }
            }
            break;
        }
    }
    run.jumps = run.notes.iter().filter(|n| n.contains("[np jump]")).count() as u32;
    run.wall_logged = run.notes.iter().filter(|n| n.contains("[np wall]")).count() as u32;
    run.moves = run.notes.iter().filter(|n| n.contains("[np move]")).count() as u32;
    run.wall_most = run
        .notes
        .iter()
        .filter_map(|n| {
            let i = n.find("a wall at most ")? + "a wall at most ".len();
            n[i..].split(' ').next()?.parse::<f32>().ok()
        })
        .fold(0.0, f32::max);

    if down_time > 0.0 {
        run.game_speed = game_moved / down_time;
    }
    // The reference's fastest second while downed (its last downed spell).
    for (i, a) in down_refs.iter().enumerate() {
        if let Some(b) = down_refs[i..].iter().find(|b| b.0 >= a.0 + 1.0) {
            let s = sub(b.1, a.1);
            run.ref_speed = run.ref_speed.max(length([s[0], s[1], 0.0]) / (b.0 - a.0));
        }
    }
    if run.handed && cfg.give_up {
        // The old get-up from the pose handed (the bodies where New Physics left them).
        let pose: Vec<(M3, [f32; 3])> = ragdoll_pose.clone();
        if std::env::var_os("NP_TRACE").is_some() {
            let mut low: Vec<(f32, &str)> = pose
                .iter()
                .zip(&f.nodes)
                .map(|(p, n)| (p.1[2], n.name.as_str()))
                .collect();
            low.sort_by(|a, b| a.0.total_cmp(&b.0));
            eprintln!("HANDED lowest {:?}", &low[..6]);
        }
        let g = crate::knockdown::fixture_tests::get_up_from(&mut f, pose);
        // Over the collision floor (the run's own): the lowest of the pelvis, head and feet (the
        // game's own get-up log's), and of every node (the fingers and thumbs too: not bodies,
        // they follow the hand's capsule through the floor).
        let (inverted, lowest) = crate::knockdown::fixture_tests::get_up_figures(&g);
        let floor_z = at[2] - cfg.below;
        let logged = crate::knockdown::fixture_tests::get_up_lowest_logged(&g);
        run.get_up = Some((inverted, logged - floor_z));
        run.get_up_any_node = lowest - floor_z;
    }
    run.clamps = crate::knockdown::CLAMPS.load(Ordering::Relaxed) - clamps0;
    room_events::set_on(false);
    AI_BUSY.store(false, Ordering::Relaxed);
    rig_fixture::SLOPE.with(|s| s.set(None));
    rig_fixture::WORLD.with(|w| *w.borrow_mut() = None);
    rig_fixture::ARMED.with(|a| a.set(false));
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    crate::gunplay::set_tuning(was);
    run
}

/// The tester's 08:13 clip's cycle on flat ground (the old get-up's own fixture's floor): a 4.8
/// m/s hit downs it; downed, no body's drawn rotation turns over 20 degrees in a frame, the
/// reference moves at most 5 units a second (and not at all by the game); it rises and stands;
/// three 0.32 m/s hits take at most one step each and never fell it, no step comes without a
/// hit, the floor carries under 3 body weights standing; a 4.8 m/s hit downs it again and its
/// rises given up hand it to the game's ragdoll: the pose inside the skeleton's limits (5
/// degrees), nothing for the knockdown's clamp to hold, drawn as simulated every frame (0.5
/// degrees, 0.5 units); the old get-up never upside down, no node more than 3 units under the
/// floor (the lowest of its pelvis, head and feet, the game's own get-up log's). The animation is
/// read only from its snapshot: never a jump.
#[test]
fn np_full_cycle_with_a_racing_animation() {
    let r = cycle_run(&Cycle {
        slope: 0.0,
        below: 1.0,
        give_up: true,
        seconds: 30.0,
        arms: 0.0,
        ..Default::default()
    });
    eprintln!("RESULT np full cycle: {r:#?}");
    assert_eq!(r.downed, 2, "{r:#?}");
    assert!(r.stood >= 1, "never stood: {r:#?}");
    assert_eq!(
        r.snaps, 0,
        "{} frames turned over 20 degrees ({:.1})",
        r.snaps, r.turn_most
    );
    assert!(
        r.ref_speed <= 5.0,
        "the reference {:.1} u/s downed",
        r.ref_speed
    );
    assert!(
        r.game_speed < 0.01,
        "the game moved it {:.2} u/s downed",
        r.game_speed
    );
    assert_eq!(r.light_hits, 3, "{r:#?}");
    assert!(!r.light_fell, "a 0.32 m/s hit fell it");
    assert!(
        r.light_steps <= 1,
        "{} steps from a 0.32 m/s hit",
        r.light_steps
    );
    assert_eq!(r.stray_steps, 0, "{} steps with no hit", r.stray_steps);
    assert!(
        r.floor_most < 3.0,
        "the floor {:.2} body weights",
        r.floor_most
    );
    assert!(
        r.drawn_off.0 <= 0.5 && r.drawn_off.1 <= 0.5,
        "drawn off {:?}",
        r.drawn_off
    );
    assert!(r.handed, "never handed over");
    assert!(
        r.handed_out <= 5.0,
        "handed {:.1} degrees outside the limits",
        r.handed_out
    );
    assert_eq!(r.clamps, 0, "the clamp held bones {} frames", r.clamps);
    let (inverted, lowest) = r.get_up.expect("no get-up");
    assert_eq!(inverted, 0, "{inverted} get-up frames upside down");
    assert!(
        lowest >= -3.0,
        "the get-up's pelvis, head or feet {lowest:.1} under the floor"
    );
    assert_eq!(r.jumps, 0, "the animation jumped {} times", r.jumps);
}

/// The walking AI on a 20 degree slope (the collision floor 7 units under the reference) with the
/// same racing animation, the game's mover and frame seconds: a 4.8 m/s hit downs it (no snap
/// over 20 degrees a frame, the reference at most 5 units a second downed, not moved by the
/// game), it stands, three 0.32 m/s hits never fell it and take at most one step each, no step
/// with no hit, the floor under 3 body weights standing, drawn as simulated.
#[test]
fn np_slope_walking_with_a_racing_animation() {
    let r = cycle_run(&Cycle {
        slope: 20.0,
        below: 7.0,
        give_up: false,
        seconds: 18.0,
        arms: 1.0,
        ..Default::default()
    });
    eprintln!("RESULT np slope cycle: {r:#?}");
    assert!(r.downed >= 1 && r.stood >= 1, "{r:#?}");
    assert_eq!(
        r.snaps, 0,
        "{} frames turned over 20 degrees ({:.1})",
        r.snaps, r.turn_most
    );
    assert!(
        r.ref_speed <= 5.0,
        "the reference {:.1} u/s downed",
        r.ref_speed
    );
    assert!(
        r.game_speed < 0.01,
        "the game moved it {:.2} u/s downed",
        r.game_speed
    );
    assert_eq!(r.light_hits, 3, "{r:#?}");
    assert!(!r.light_fell, "a 0.32 m/s hit fell it");
    assert!(
        r.light_steps <= 1,
        "{} steps from a 0.32 m/s hit",
        r.light_steps
    );
    assert_eq!(r.stray_steps, 0, "{} steps with no hit", r.stray_steps);
    assert!(
        r.floor_most < 3.0,
        "the floor {:.2} body weights",
        r.floor_most
    );
    assert!(
        r.drawn_off.0 <= 0.5 && r.drawn_off.1 <= 0.5,
        "drawn off {:?}",
        r.drawn_off
    );
    assert_eq!(r.jumps, 0, "the animation jumped {} times", r.jumps);
}

/// A box about the reference (units): `x` and `y` its extents, standing `h` on the collision
/// floor `below` under the reference.
fn block(x: (f32, f32), y: (f32, f32), below: f32, h: f32) -> (V3, V3) {
    ([x.0, y.0, -below - 20.0], [x.1, y.1, -below + h])
}

/// What [`barrier_runs`] found wrong (none: all held).
fn barrier_runs(worlds: Vec<(&str, Vec<(V3, V3)>, bool)>) -> Vec<String> {
    let mut failed = Vec::new();
    for (name, boxes, wall) in worlds {
        let r = cycle_run(&Cycle {
            below: 1.0,
            seconds: 14.0,
            boxes,
            ..Default::default()
        });
        eprintln!(
            "RESULT np barrier {name}: downed {}, stood {} (first after {:?} s), the deepest body \
             inside {:.2} u ({}), a wall at most {:.0} N, {} [np wall] lines, the floor at most \
             {:.2} body weights standing, the fastest body past the hit {:.2} m/s, the reference \
             inside {:.2} u, moves swept {}",
            r.downed,
            r.stood,
            r.first_stood,
            r.inside_most,
            r.inside_at,
            r.wall_most,
            r.wall_logged,
            r.floor_most,
            r.fast_most,
            r.ref_inside,
            r.moves
        );
        if r.downed == 0 || r.stood == 0 {
            failed.push(format!("{name}: downed {}, stood {}", r.downed, r.stood));
        }
        if r.inside_most > 2.0 {
            failed.push(format!("{name}: a body {:.2} u inside", r.inside_most));
        }
        if wall && (r.wall_most <= 0.0 || r.wall_logged == 0) {
            failed.push(format!(
                "{name}: a wall's load {:.0} N, {} lines",
                r.wall_most, r.wall_logged
            ));
        }
        if r.floor_most >= 3.0 {
            failed.push(format!(
                "{name}: the floor {:.2} body weights",
                r.floor_most
            ));
        }
        if r.fast_most > 6.0 {
            failed.push(format!("{name}: a body at {:.2} m/s", r.fast_most));
        }
        if r.ref_inside > 1.0 {
            failed.push(format!(
                "{name}: the reference {:.2} u inside",
                r.ref_inside
            ));
        }
    }
    failed
}

/// Patrick's 10:08 clip: knocked down across a 12 unit slab's edge (the walking AI, the game's
/// mover, the racing animation, frame seconds 0.008 to 0.038, FNV's skeleton): no body more than
/// 2 units inside it (one plane through the samples tilted at its edge), the floor under 3 body
/// weights standing, no body faster than 6 m/s but in the fall the hit causes, the reference's
/// capsule never in it; it stands again.
#[test]
fn np_a_slab_edge_holds_the_body() {
    let failed = barrier_runs(vec![(
        "slab",
        vec![block((-150.0, 150.0), (-300.0, -48.0), 1.0, 12.0)],
        false,
    )]);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Patrick's 10:08 clip: knocked down into a wall and against a crate: no body more than 2 units
/// inside either and a wall's load and its `[np wall]` line (both hold now), but standing again
/// by them is open: by the wall the floor reads 3.5 body weights once standing, by the crate its
/// rises are given up (three) and it goes to the game's ragdoll.
#[test]
#[ignore = "open: rising beside a wall or a crate (see the doc)"]
fn np_walls_and_crates_hold_the_body() {
    let failed = barrier_runs(vec![
        (
            "wall",
            vec![block((-120.0, 120.0), (-62.0, -50.0), 1.0, 150.0)],
            true,
        ),
        (
            "crate",
            vec![block((-22.0, 22.0), (-76.0, -40.0), 1.0, 50.0)],
            true,
        ),
    ]);
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Downed by a wall off the reference (a 4.8 m/s hit carries it back and aside, against a wall):
/// the reference's move to it at the rise's end is swept (`[np move]` when it meets or nudges
/// off something) and the actor's capsule never lies in the wall, ending clear of it; it stands.
/// Open: lying against the wall its rises are given up, so the move is never made.
#[test]
#[ignore = "open: rising beside a wall (see the doc)"]
fn np_the_reference_moves_clear_of_a_wall() {
    let below = 1.0;
    let r = cycle_run(&Cycle {
        below,
        seconds: 12.0,
        knock: [0.45, -0.89, 0.0],
        boxes: vec![block((75.0, 89.0), (-300.0, 60.0), below, 150.0)],
        ..Default::default()
    });
    eprintln!(
        "RESULT np reference by a wall: downed {}, stood {}, the reference furthest at ({:.1}, {:.1}), \
         at most {:.2} u inside the wall (the capsule's radius in), moves swept {}, the deepest body \
         inside {:.2} u ({}); {:?}",
        r.downed, r.stood, r.ref_end[0], r.ref_end[1], r.ref_inside, r.moves, r.inside_most,
        r.inside_at,
        r.notes.iter().filter(|n| n.contains("[np move]")).collect::<Vec<_>>()
    );
    assert!(r.downed >= 1 && r.stood >= 1, "{r:#?}");
    assert!(r.ref_end[0] < 75.0, "through the wall: {:?}", r.ref_end);
    assert!(
        r.ref_inside <= 1.0,
        "the capsule {:.2} u into the wall",
        r.ref_inside
    );
    assert!(r.inside_most <= 2.0, "a body {:.2} u inside", r.inside_most);
    assert!(
        r.ref_end[1] < -60.0,
        "the reference never went to the body: furthest {:?}",
        r.ref_end
    );
}

/// Patrick's 10:10 clip: the whole cycle at 4.6 m/s on flat ground and beside a 12 unit slab:
/// it stands within 8 s of going down on its first try (no rise given up), never handed to the
/// game's ragdoll, and once upright it is still (its centre under 0.4 m/s against its ground)
/// within 1 s.
#[test]
fn np_rises_on_its_first_try() {
    let below = 1.0;
    let mut failed = Vec::new();
    for (name, boxes) in [
        ("flat", Vec::new()),
        (
            "by the slab",
            vec![block((40.0, 200.0), (-300.0, 300.0), below, 12.0)],
        ),
    ] {
        let r = cycle_run(&Cycle {
            below,
            seconds: 12.0,
            heavy: 4.6,
            boxes,
            ..Default::default()
        });
        let given_up = r
            .notes
            .iter()
            .any(|n| n.contains("[np choice]") && !n.contains("rises given up 0"));
        eprintln!(
            "RESULT np first try {name}: downed {}, stood after {:?} s, a rise given up {given_up}, \
             upright to still {:.2} s, handed {}, clamps {}",
            r.downed, r.first_stood, r.upright_still, r.handed, r.clamps
        );
        if r.downed == 0 || !r.first_stood.is_some_and(|s| s <= 8.0) {
            failed.push(format!("{name}: stood after {:?} s", r.first_stood));
        }
        if given_up {
            failed.push(format!("{name}: a rise given up"));
        }
        if r.handed {
            failed.push(format!("{name}: handed to the game's ragdoll"));
        }
        if r.upright_still > 1.0 {
            failed.push(format!(
                "{name}: {:.2} s upright before still",
                r.upright_still
            ));
        }
        if r.clamps != 0 {
            failed.push(format!("{name}: {} clamps", r.clamps));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Patrick's 10:08 clip: light hits of 0.25 to 0.74 m/s and 7-pellet bursts (one frame, 0.5
/// m/s in all) on the walking AI: none falls it, no fall with the capture point 0.15 m or less
/// outside the feet; every catch at least 15 cm and its landing covering the capture point; each
/// burst one outcome of its 7 pellets (none "stood" over no time).
#[test]
fn np_light_hits_and_bursts_step_right() {
    let mut failed = Vec::new();
    for (light, pellets) in [(0.25f32, 1u32), (0.5, 1), (0.74, 1), (0.5, 7)] {
        let r = cycle_run(&Cycle {
            below: 1.0,
            seconds: 14.0,
            light,
            pellets,
            ..Default::default()
        });
        let outcomes: Vec<&String> = r
            .notes
            .iter()
            .filter(|n| n.contains("[np outcome]") && n.contains(&format!("/ {:.2} m/s", light)))
            .collect();
        let low_falls: Vec<&String> = r
            .notes
            .iter()
            .filter(|n| n.contains("[np fall] steps"))
            .filter(|n| {
                n.split("the capture point ")
                    .nth(1)
                    .and_then(|x| x.split(' ').next())
                    .and_then(|x| x.parse::<f32>().ok())
                    .is_some_and(|o| o <= 0.15)
            })
            .collect();
        eprintln!(
            "RESULT np light {light} m/s x{pellets}: hits {}, fell {}, catches {:?}, outcomes {:?}",
            r.light_hits,
            r.light_fell,
            r.catches
                .iter()
                .map(|c| format!("{:.2} m / {:.3} m out", c.0, c.1))
                .collect::<Vec<_>>(),
            outcomes
                .iter()
                .map(|o| o
                    .split(" -> ")
                    .nth(1)
                    .unwrap_or("")
                    .chars()
                    .take(40)
                    .collect::<String>())
                .collect::<Vec<_>>()
        );
        if r.light_hits == 0 || r.light_fell || !low_falls.is_empty() {
            failed.push(format!(
                "{light} x{pellets}: hits {}, fell {}, {low_falls:?}",
                r.light_hits, r.light_fell
            ));
        }
        for &(reach, covers) in &r.catches {
            if reach < ragdoll_core::np_balance::STEP_LEAST - 1e-3
                || covers > ragdoll_core::np_balance::MARGIN
            {
                failed.push(format!(
                    "{light} x{pellets}: a catch {reach:.3} m, {covers:.3} m out"
                ));
            }
        }
        if outcomes.len() < r.light_hits as usize {
            failed.push(format!(
                "{light} x{pellets}: {} outcomes for {} hits",
                outcomes.len(),
                r.light_hits
            ));
        }
        for o in &outcomes {
            if !o.contains(&format!("({pellets} pellets)")) || o.contains("over 0.00 s") {
                failed.push(format!("{light} x{pellets}: {o}"));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The get-ups the arms tests run: down on its face and on its back (falling forward, back), on
/// flat ground and on a 20 degree slope, unarmed and with a weapon drawn.
fn arms_runs() -> Vec<(&'static str, bool, CycleRun)> {
    [
        ("on its face, flat", [0.0, 1.0, 0.0], 0.0f32, false),
        ("on its back, flat", [0.0, -1.0, 0.0], 0.0, false),
        ("on its face, slope", [0.0, 1.0, 0.0], 20.0, false),
        ("on its back, slope", [0.0, -1.0, 0.0], 20.0, false),
        ("on its back, armed", [0.0, -1.0, 0.0], 0.0, true),
    ]
    .into_iter()
    .map(|(name, knock, slope, armed)| {
        let r = cycle_run(&Cycle {
            slope,
            below: if slope > 0.0 { 7.0 } else { 1.0 },
            seconds: 10.0,
            knock,
            armed,
            ..Default::default()
        });
        eprintln!(
            "RESULT np arms {name}: downed {}, stood after {:?} s, hands planted {} frames, \
             carrying at most {:.2} of its weight, slid {:.2} u, bounced {}, rose {:.2} u, the \
             weapon hand planted {} frames",
            r.downed,
            r.first_stood,
            r.planted_frames,
            r.hand_share,
            r.hand_slip,
            r.hand_bounce,
            r.hand_lift,
            r.weapon_planted
        );
        (name, armed, r)
    })
    .collect()
}

/// The arms' push in a get-up (Patrick's 10:08 and 10:10 clips: the arms passive at its sides),
/// in each of [`arms_runs`]: the hands put down carry a real share of the body's weight while the
/// torso comes up (a tenth or more for each arm that pushes: both unarmed, the free one armed),
/// the weapon's hand never pushes (the free arm reaches), and it stands.
#[test]
fn np_the_arms_push_it_up() {
    let mut failed = Vec::new();
    for (name, armed, r) in arms_runs() {
        if r.downed == 0 || r.first_stood.is_none() {
            failed.push(format!(
                "{name}: downed {}, stood {:?}",
                r.downed, r.first_stood
            ));
        }
        let need = if armed { 0.1 } else { 0.2 };
        if r.hand_share < need {
            failed.push(format!("{name}: the hands carried {:.2}", r.hand_share));
        }
        if armed && r.weapon_planted > 0 {
            failed.push(format!(
                "{name}: the weapon hand planted {}",
                r.weapon_planted
            ));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A planted hand never slides (2 units at its node) nor bounces (its load gone and back while
/// planted) in [`arms_runs`]. Open: the torso's move toward the feet drags a hand planted beside
/// the shoulder 14 to 35 units across the ground (its hold 3000 N/m, at most 400 N), and on the
/// slope a hand's load comes and goes 3 to 8 times; a stiffer hold and a let-go when dragged or
/// lifted stopped both but changed the rises enough to break the cycle's own tests.
#[test]
#[ignore = "open: planted hands slide and, on a slope, bounce (see the doc)"]
fn np_planted_hands_hold_still() {
    let bad: Vec<String> = arms_runs()
        .into_iter()
        .filter(|(_, _, r)| r.hand_slip > 2.0 || r.hand_bounce > 0)
        .map(|(name, _, r)| {
            format!(
                "{name}: slid {:.2} u, bounced {}",
                r.hand_slip, r.hand_bounce
            )
        })
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Patrick's addendum 2 (his 13:17 note: the cripple reaction still played). The game's
/// one-off reaction to a limb crippled by a hit (`actor_p3::hit_me_cripple_idle`, the section of
/// `Actor::HitMe` that asks `TESIdleManager::GetIdleToPlay` for an idle and sets it as the
/// process's, `HighProcess::SetCurrentProcessIdle`) runs, the game's own, inside the hooked hit
/// (`knockdown::hit_me_hooked`, `Setting::GetValue`'s address-of hooked as the game is) on a real
/// `Setting` at `011df760` (its vtable word and its value 1 at `+4`): a leg's crippling hit on
/// the main thread, then an arm's on an AI thread. With New Physics on neither asks for an idle
/// nor sets one; with it off each does both. The setting's words are unchanged either way, and the
/// limp (the game's `Actor::ConditionModifiedCallback` on the legs' value 0x48 crippled: its
/// replacement locomotion requested) comes as before.
#[test]
fn np_crippling_hits_never_start_the_cripple_reaction() {
    use engine::units::fallout_ai::actor_p3::hit_me_cripple_idle;
    use engine::units::fallout_ai::actor_p6::actor_condition_modified_callback;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    static AI_THREAD: AtomicBool = AtomicBool::new(false);
    static ASKED: AtomicU32 = AtomicU32::new(0);
    static SET: AtomicU32 = AtomicU32::new(0);
    static LIMP: AtomicU32 = AtomicU32::new(0);
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    // The game's data: the setting, the idle manager's pointer, the static byte, 2.0.
    const SETTING_VTABLE: u32 = 0x0102_5b84;
    const PAGE: u32 = 0x7400_0000;
    const HIT: u32 = PAGE;
    const OWNER_VTABLE: u32 = PAGE + 0x400;
    const PROCESS_VTABLE: u32 = PAGE + 0x800;
    const IDLE: u32 = PAGE + 0xc00;
    let mut failed = Vec::new();
    for on in [true, false] {
        let mut f = Fixture::new(
            rig_fixture::fnv_skeleton(),
            Ground {
                collision: Some(-1.0),
                navmesh: -9.0,
            },
        );
        f.game_update();
        f.make_character();
        crate::active_ragdoll::clear();
        crate::knockdown::clear();
        crate::knockdown::install_setting_value(&mut f.e);
        let was = crate::gunplay::tuning();
        crate::gunplay::set_tuning(crate::gunplay::Tuning {
            new_physics: on,
            knockdowns: false,
            ..was
        });
        for page in [
            0x011d_f000u32,
            0x011c_b000,
            0x0120_2000,
            0x0101_1000,
            0x011d_9000,
            PAGE,
        ] {
            f.e.mem.map(page, 0x1000);
        }
        f.e.mem.set_u32(CRIPPLE_IDLE, SETTING_VTABLE);
        f.e.mem.set_u32(CRIPPLE_IDLE + 4, 1);
        f.e.mem.set_u32(0x011c_b6a0, PAGE + 0xf00);
        f.e.mem.set_f64(0x0101_1590, 2.0);
        f.e.register_double(0x008c_7aa0, |_, _| {
            u32::from(AI_THREAD.load(Ordering::Relaxed)).into_ret()
        });
        f.e.register_double(0x0088_45a0, |_, _| 1u32.into_ret());
        // The idle manager's answer, and the process's idle set: counted.
        ASKED.store(0, Ordering::Relaxed);
        SET.store(0, Ordering::Relaxed);
        LIMP.store(0, Ordering::Relaxed);
        f.e.register_double(0x0060_0950, |_, _| {
            ASKED.fetch_add(1, Ordering::Relaxed);
            IDLE.into_ret()
        });
        f.e.register_double(0x0049_8d30, |_, _| 0u32.into_ret());
        let vt = f.e.mem.u32(ACTOR);
        // Actor slots: `+0x230` false (it may react), `+0x1e4` its animation, `+0x360` false.
        f.e.register_double(PAGE + 0xe00, |_, _| 0u32.into_ret());
        f.e.register_double(PAGE + 0xe04, |_, _| (PAGE + 0xd00).into_ret());
        f.e.mem.set_u32(vt + 0x230, PAGE + 0xe00);
        f.e.mem.set_u32(vt + 0x1e4, PAGE + 0xe04);
        f.e.mem.set_u32(vt + 0x360, PAGE + 0xe00);
        // Its actor values (`+0xa4`): 0x48, the legs' mobility, at 0 (crippled).
        f.e.mem.set_u32(ACTOR + 0xa4, OWNER_VTABLE);
        f.e.register_double(PAGE + 0xe08, |_, _| 0u32.into_ret());
        f.e.register_double(PAGE + 0xe0c, |_, _| Ret {
            st0: 0.0,
            ..Ret::default()
        });
        f.e.mem.set_u32(OWNER_VTABLE + 0x08, PAGE + 0xe08);
        f.e.mem.set_u32(OWNER_VTABLE + 0x0c, PAGE + 0xe0c);
        // Its process's `SetCurrentProcessIdle` (`+0x71c`) and `+0x614`.
        let process = f.e.mem.u32(ACTOR + rig_fixture::PROCESS);
        let pvt = match f.e.mem.u32(process) {
            0 => {
                f.e.mem.set_u32(process, PROCESS_VTABLE);
                PROCESS_VTABLE
            }
            v => v,
        };
        f.e.register_double(PAGE + 0xe10, |_, _| {
            SET.fetch_add(1, Ordering::Relaxed);
            Ret::default()
        });
        f.e.mem.set_u32(pvt + 0x71c, PAGE + 0xe10);
        f.e.mem.set_u32(pvt + 0x614, PAGE + 0xe00);
        // The limp: the legs' conditions at 0, the locomotion change asked for.
        f.e.register_double(0x0064_6800, |_, _| Ret {
            st0: 0.0,
            ..Ret::default()
        });
        f.e.register_double(0x008b_7360, |_, _| {
            LIMP.fetch_add(1, Ordering::Relaxed);
            Ret::default()
        });
        f.e.register_double(0x0088_48e0, |_, _| Ret::default());
        room_events::set_on(true);
        let _ = room_events::take();
        let mut asked = Vec::new();
        let mut set = Vec::new();
        let mut kept = on;
        let mut jumps = 0;
        for (thread, limb) in [(false, 6u32), (true, 3)] {
            f.e.set(
                engine::Ptr::<engine::units::fallout_ai::hitdata::HitData>::new(HIT),
                engine::units::fallout_ai::hitdata::HitData::eDamageLimb,
                limb,
            );
            f.e.mem.set_u32(HIT + crate::knockdown::HIT_TARGET, ACTOR);
            let (a0, s0) = (ASKED.load(Ordering::Relaxed), SET.load(Ordering::Relaxed));
            AI_THREAD.store(thread, Ordering::Relaxed);
            crate::knockdown::hit_me_hooked(&mut f.e, &[ACTOR, HIT, 0], |e, a| {
                hit_me_cripple_idle(
                    e,
                    engine::Ptr::new(a[0]),
                    engine::Ptr::new(a[1]),
                    0,
                    0,
                    true,
                )
            });
            AI_THREAD.store(false, Ordering::Relaxed);
            asked.push(ASKED.load(Ordering::Relaxed) - a0);
            set.push(SET.load(Ordering::Relaxed) - s0);
            for _ in 0..20 {
                f.e.mem.set_f32(0x011d_ea30, 0.016);
                main_thread(&mut f.e);
                f.animate();
                crate::active_ragdoll::apply(&mut f.e);
                crate::active_ragdoll::read_drawn(&mut f.e);
                crate::active_ragdoll::after_draw(&mut f.e);
                crate::active_ragdoll::unwrite(&mut f.e);
                if on && !owns(ACTOR) {
                    kept = false;
                }
            }
            jumps += room_events::take()
                .iter()
                .filter(|l| l.contains("[np jump]"))
                .count();
        }
        room_events::set_on(false);
        let words = (f.e.mem.u32(CRIPPLE_IDLE), f.e.mem.u32(CRIPPLE_IDLE + 4));
        actor_condition_modified_callback(
            &mut f.e,
            engine::Ptr::new(ACTOR + 0xa4),
            0x48,
            1.0,
            -1.0,
            engine::Ptr::new(0),
        );
        let limp = LIMP.load(Ordering::Relaxed);
        eprintln!(
            "RESULT np cripple reaction, New Physics {on}: idles asked for {asked:?}, set {set:?} \
             (the leg's hit on the main thread, the arm's on an AI thread); the setting's words \
             {:#x} {:#x}; the limp's locomotion asked {limp}; kept by New Physics {kept}, \
             animation jumps {jumps}",
            words.0, words.1
        );
        let want = if on { [0, 0] } else { [1, 1] };
        if asked != want || set != want {
            failed.push(format!("on {on}: asked {asked:?}, set {set:?}"));
        }
        if words != (SETTING_VTABLE, 1) {
            failed.push(format!("on {on}: the setting's words {words:x?}"));
        }
        if limp == 0 {
            failed.push(format!("on {on}: no limp"));
        }
        if on && (!kept || jumps != 0) {
            failed.push(format!("on {on}: kept {kept}, jumps {jumps}"));
        }
        if !on && owns(ACTOR) {
            failed.push("off: taken".to_string());
        }
        crate::active_ragdoll::clear();
        crate::knockdown::clear();
        crate::gunplay::set_tuning(was);
    }
    assert!(failed.is_empty(), "{failed:#?}");
}
