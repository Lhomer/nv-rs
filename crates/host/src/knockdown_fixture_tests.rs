//! The knockdown's get-up run on an actor's 3D built from a node tree (`crate::rig_fixture`):
//! the ragdoll's bodies on the tree's nodes (none on the pelvis, as in the game: Patrick's a7a2d15
//! clips), the body lying on the floor, then [`get_ups`] each frame as the game's own runs it,
//! through the knock states to the get-up animation (knock state 6) and its drawn blend
//! (`crate::active_ragdoll`'s write, read back from the nodes).

use super::*;
use crate::rig_fixture::{self, Fixture, Ground, ACTOR, ACTOR_ID};
use ragdoll_core::flinch::{length, rotation, sub, IDENTITY};

/// One at a time with the reactions' own tests: the knockdowns' and the reactions' lists are the
/// process's.
use crate::active_ragdoll::tests::SERIAL;

/// A knock-down's get-up bookkeeping for the fixture's actor, as `knock_down` leaves it, the body
/// lying for `lying` seconds already.
fn knocked(stood: [f32; 3], lying: f32) {
    let since = Instant::now() - Duration::from_secs_f32(lying + FLIGHT.as_secs_f32());
    let mut ups = GET_UPS.lock().unwrap_or_else(|p| p.into_inner());
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

/// What a get-up did: the actor's reference once set, and each frame of the get-up's drawing
/// (seconds, the drawn joints of every rig bone).
struct Run {
    set_at: Option<[f32; 3]>,
    pelvis_found: bool,
    blend_from: f32,
    frames: Vec<(f32, Vec<[f32; 3]>)>,
    lowest_drawn: f32,
    fastest_rise: f32,
}

/// The tree `nodes` lying on its back (its pelvis `over` units over the collision floor at 0, the
/// navmesh `navmesh` under it), knocked down where it stood at (0, 0, `stood`); `get_ups` and the
/// reactions' frames run until the get-up animation (standing) has drawn for a second.
fn get_up(nodes: Vec<rig_fixture::Node>, over: f32, navmesh: f32, stood: f32) -> Run {
    let mut f = Fixture::new(
        nodes,
        Ground {
            collision: Some(0.0),
            navmesh,
        },
    );
    crate::active_ragdoll::clear();
    clear();
    rig_fixture::write3(
        &mut f.e,
        ACTOR + rig_fixture::REFR_POSITION,
        [0.0, 0.0, stood],
    );
    let pelvis_standing = f.world("Bip01 Pelvis").expect("a pelvis").1;
    // On its back: turned a quarter about the side axis (its head toward -Y), the pelvis `over`
    // units over the floor.
    let back = rotation([std::f32::consts::FRAC_PI_2, 0.0, 0.0]);
    f.ragdoll_pose(back, pelvis_standing, [0.0, 0.0, over - pelvis_standing[2]]);
    f.set_knock_state(1);
    knocked([0.0, 0.0, stood], 1.0);
    let mut run = Run {
        set_at: None,
        pelvis_found: false,
        blend_from: 0.0,
        frames: Vec::new(),
        lowest_drawn: f32::MAX,
        fastest_rise: 0.0,
    };
    // Lying still until it is let up.
    for _ in 0..60 {
        get_ups(&mut f.e);
        if let Some(g) = get_up_of() {
            if let Some((_, at)) = g.placed {
                run.set_at = Some(at);
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let bones = bodies(&mut f.e, ACTOR);
    run.pelvis_found = pelvis_pos(&mut f.e, ACTOR, &bones).is_some();
    // The game's get-up: states 3 and 5 a frame each (the bodies still lying), then 6: the get-up
    // animation (standing at the reference it was set at) has the locals, and the physics pushes
    // the bodies up out of the floor (the depenetration a7a2d15 drew as a handstand).
    for state in [3u8, 5] {
        f.set_knock_state(state);
        get_ups(&mut f.e);
    }
    let reference = f.reference();
    f.set_knock_state(6);
    let root = f.at[0];
    rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, reference);
    f.animate();
    // The animated (standing) drawing, the drawn pose at 6's first frame.
    let anim: Vec<[f32; 3]> = ragdoll_core::flinch::BONES
        .iter()
        .filter_map(|(n, _)| f.world(n).map(|w| w.1))
        .collect();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1100) {
        std::thread::sleep(Duration::from_millis(16));
        f.set_body_velocities([0.0, 0.0, 300.0]);
        get_ups(&mut f.e);
        let up = f
            .body_velocities()
            .iter()
            .map(|(_, v)| v[2])
            .fold(0.0, f32::max);
        run.fastest_rise = run.fastest_rise.max(up);
        f.animate();
        crate::active_ragdoll::apply(&mut f.e);
        let drawn: Vec<[f32; 3]> = ragdoll_core::flinch::BONES
            .iter()
            .filter_map(|(n, _)| f.world(n).map(|w| w.1))
            .collect();
        if run.frames.is_empty() {
            run.blend_from = drawn
                .iter()
                .zip(&anim)
                .map(|(a, b)| length(sub(*a, *b)))
                .fold(0.0, f32::max);
        }
        run.lowest_drawn = drawn.iter().map(|p| p[2]).fold(run.lowest_drawn, f32::min);
        run.frames.push((start.elapsed().as_secs_f32(), drawn));
        crate::active_ragdoll::after_draw(&mut f.e);
        crate::active_ragdoll::unwrite(&mut f.e);
    }
    crate::active_ragdoll::clear();
    clear();
    let _ = IDENTITY;
    run
}

fn fastest(frames: &[(f32, Vec<[f32; 3]>)]) -> f32 {
    frames
        .windows(2)
        .map(|w| {
            let dt = (w[1].0 - w[0].0).max(1e-3);
            w[1].1
                .iter()
                .zip(&w[0].1)
                .map(|(a, b)| length(sub(*a, *b)) / dt)
                .fold(0.0, f32::max)
        })
        .fold(0.0, f32::max)
}

/// The get-up on the mannequin laid out as FNV's tree (no body on the pelvis), lying 9 units over
/// a collision floor at 0 with the navmesh 8 under it, knocked down where it stood on that floor:
/// the pelvis is found, the actor is set on the collision floor (never the navmesh, never the
/// lowest bone), the blend starts from the ragdoll's pose (its farthest bone well off the
/// animation's) and eases (no bone drawn over 400 units a second), no body rises faster than 50
/// units a second through the get-up, and nothing is drawn under the floor by more than 3.
#[test]
fn the_get_up_finds_its_pelvis_sets_it_on_the_floor_and_blends() {
    let _one = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let run = get_up(rig_fixture::mannequin(), 9.0, -8.0, 0.0);
    let set = run.set_at.expect("never let up");
    assert!(run.pelvis_found, "no pelvis found");
    assert!(
        set[2].abs() < 0.5,
        "set at z {:.1}, not on the collision floor at 0",
        set[2]
    );
    assert!(
        run.blend_from > 20.0,
        "the blend starts {:.1} off the animation",
        run.blend_from
    );
    let v = fastest(&run.frames);
    assert!(v <= 400.0, "a bone drawn at {v:.0} units/s in the get-up");
    assert!(
        run.fastest_rise <= 50.0 + 0.5,
        "a body rising {:.0} units/s",
        run.fastest_rise
    );
    assert!(
        run.lowest_drawn >= -3.0,
        "drawn {:.1} under the floor",
        run.lowest_drawn
    );
}
