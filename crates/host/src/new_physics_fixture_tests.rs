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
        "RESULT np on FNV's skeleton 0.8 m/s: read-back {:.3} deg / {:.3} u, put back {:.3} u, spine peak {:.1} deg, {} catching steps, moved ({:.1}, {:.1}), lowest {:.1} u, step most {:.1} u / {:.1} deg",
        r.readback.0, r.readback.1, r.put_back, r.spine_peak, r.steps, r.moved[0], r.moved[1], r.lowest, r.step_most.0, r.step_most.1
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
    assert_eq!(r.steps, 0);
    assert!(!owns(ACTOR));
}
