//! The hit reactions' replays in CI (`crate::physics_replay`): each scenario run through New
//! Physics' own path on the tests' mannequin (`crate::rig_fixture`, FNV's tree laid out, no game
//! file) with Delirium's stand-in body (`crate::delirium_stand_in`), watched as the game's
//! self-test watches it, and judged by the same checks (`crate::physics_checks`). The checks that
//! count here are the integration's own (`Where::Both`); what only Delirium's own body can show
//! (`Where::Pc`) is said as INFO. A replay recorded by the game's self-test on the player's PC runs
//! the same way: `NV_PHYSICS_REPLAY=<file> cargo test -p host --lib replay_from_the_file --
//! --nocapture`.

use super::*;
use crate::physics_checks::{self, Frame};
use crate::physics_replay::{self, Scenario};
use crate::rig_fixture::{self, Fixture, Ground, ACTOR};

/// Where the tests' actor's base form and ragdoll controller are made up (inside the fixture's
/// memory, past what it lays out).
const FAKE_BASE: u32 = 0x0200_e000;
const FAKE_CONTROLLER: u32 = 0x0200_e800;

/// Failures found by the replay and open (each its scenario, its check and why, reported to
/// Patrick): tracked here, never hidden. The test fails on any other failure, and on one of these
/// passing (it is then fixed: take it off the list). None open now (the slope's hand-back to the
/// AI was fixed with the legs' blend, `RELEASE_FOR`).
const KNOWN_OPEN: &[(&str, &str)] = &[];

/// `s` run: its checks' lines and whether every one that counts here passed.
fn replay(s: &Scenario) -> (Vec<String>, bool) {
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
    // A human NPC (its base form an NPC's), with a ragdoll controller its corpse can be held by.
    f.e.mem.set_u32(ACTOR + 0x20, FAKE_BASE);
    f.e.mem.set_u8(FAKE_BASE + 4, 0x2a);
    f.e.mem.set_f32(FAKE_BASE + 0x1f8, 0.5);
    f.e.mem.set_u32(ACTOR + 0xac, FAKE_CONTROLLER);
    // The ground: the fixture's floor, tilted as the replay's, met by every cast.
    let at = f.reference();
    rig_fixture::SLOPE.with(|c| c.set(Some([at[0], at[1], s.slope[0], s.slope[1]])));
    rig_fixture::BUMPS.with(|c| {
        c.set((s.bumps[0] > 0.0).then_some([at[0], at[1], s.bumps[0], s.bumps[1]]))
    });
    rig_fixture::WORLD.with(|w| *w.borrow_mut() = Some(Vec::new()));
    // A hand-over to the game's ragdoll, should one come: answered as the game would.
    f.e.register_double(0x008c_7aa0, |_, _| 0u32.into_ret());
    f.e.register_double(0x0088_45a0, |_, _| 1u32.into_ret());
    f.e.register_double(crate::hit_force::KNOCK_EXPLOSION, |e, a| {
        e.mem.set_u8(a[0] + 0x13c, 2);
        0u32.into_ret()
    });
    crate::delirium_stand_in::set_on(true);
    let havok_before = to_havok(ACTOR);
    let body_nodes: Vec<usize> = model()
        .bodies
        .iter()
        .map(|b| f.nodes.iter().position(|n| n.name == b.node).unwrap())
        .collect();
    let root = f.at[0];
    f.animate();
    let keyframe = |f: &mut Fixture| {
        for (node, body) in crate::knockdown::tests_bodies(&mut f.e, ACTOR) {
            let (r, t) = f.world_of(node);
            rig_fixture::write3(&mut f.e, body + 0x120, scale(t, 1.0 / rig_fixture::HK));
            let q = ragdoll_core::active::qfrom_m(&r);
            for (i, x) in [q[1], q[2], q[3], q[0]].into_iter().enumerate() {
                f.e.mem.set_f32(body + 0x160 + 4 * i as u32, x);
            }
        }
    };
    keyframe(&mut f);
    assert!(claim(&mut f.e, ACTOR), "{}: not claimed", s.name);
    let floor = rig_fixture::floor_at(f.reference()).unwrap_or(0.0);
    let anim: Vec<(M3, V3)> = body_nodes.iter().map(|&i| f.world_of(f.at[i])).collect();
    let standing = if s.standing > 0.0 {
        s.standing
    } else {
        physics_checks::stance(&anim, floor).map_or(67.0, |p| p.0)
    };
    let mut frames = Vec::with_capacity(s.dt.len());
    let mut died = false;
    let mut t = 0.0;
    for (i, &dt) in s.dt.iter().enumerate() {
        t += dt;
        f.e.mem.set_f32(0x011d_ea30, dt);
        main_thread(&mut f.e);
        let reference = f.reference();
        rig_fixture::write3(&mut f.e, root + rig_fixture::LOCAL_TRANSLATION, reference);
        f.animate();
        if !died {
            keyframe(&mut f);
        }
        let mut struck = 0.0f32;
        for (_, body, p, dir) in s.hits.iter().filter(|h| h.0 == i) {
            let b = model().body(body).unwrap_or(model().root);
            let point = f.world_of(f.at[body_nodes[b]]).1;
            hit_shot(&mut f.e, ACTOR, *p, point, *dir, false);
            struck = struck.max(*p);
        }
        if s.kill == Some(i) && !died {
            died = true;
            f.e.mem.set_u32(ACTOR + 0x108, 1);
        }
        crate::active_ragdoll::apply(&mut f.e);
        crate::active_ragdoll::read_drawn(&mut f.e);
        let p = watch(ACTOR);
        let drawn = p.as_ref().map(|p| p.drawn.clone()).unwrap_or_default();
        frames.push(Frame {
            t,
            dt,
            ground: drawn
                .iter()
                .map(|b| rig_fixture::floor_at(b.1).unwrap_or(floor))
                .collect(),
            drawn,
            hit: struck,
            rising: p.as_ref().is_some_and(|p| p.down && p.rise),
            floor: rig_fixture::floor_at(f.reference()).unwrap_or(floor),
            owner: p.as_ref().map_or("animation", |p| p.owner),
            havok: p.as_ref().is_some_and(|p| p.handing.is_some()),
            wall: p.as_ref().map_or(0.0, |p| p.wall_most),
            floor_load: p.as_ref().map_or(0.0, |p| p.floor_most),
            dead: died,
        });
    }
    // Who had it, each change (with the pelvis over the floor): the run's story, beside its lines.
    let mut last = String::new();
    for f in &frames {
        let rising = if f.rising { " (rising)" } else { "" };
        let tag = format!("{}{rising}", f.owner);
        if tag != last {
            let up = physics_checks::stance(&f.drawn, f.floor).map_or(-1.0, |p| p.0);
            println!("  {}: {:.2} s {tag}, the pelvis {up:.0} u up", s.name, f.t);
            last = tag;
        }
    }
    crate::delirium_stand_in::set_on(false);
    rig_fixture::SLOPE.with(|c| c.set(None));
    rig_fixture::BUMPS.with(|c| c.set(None));
    rig_fixture::WORLD.with(|w| *w.borrow_mut() = None);
    crate::gunplay::set_tuning(was);
    let mut expect = s.expect();
    expect.standing = standing;
    let checks = physics_checks::judge(&frames, &expect, to_havok(ACTOR) - havok_before, None);
    physics_checks::lines(&s.name, &checks, false)
}

/// Every scenario of `scenarios` run, its lines printed; the failures not on [`KNOWN_OPEN`]
/// (`scenario: check`), and those on it that passed.
fn run_all(scenarios: &[Scenario]) -> (Vec<String>, Vec<String>) {
    let (mut failed, mut fixed) = (Vec::new(), Vec::new());
    for s in scenarios {
        let (lines, _) = replay(s);
        for l in &lines {
            println!("{l}");
            let known = KNOWN_OPEN
                .iter()
                .any(|(sc, c)| *sc == s.name && l.contains(&format!("{sc}: {c} (")));
            if l.starts_with("FAIL") && !known {
                failed.push(l.clone());
            }
            if l.starts_with("PASS") && known {
                fixed.push(l.clone());
            }
        }
    }
    (failed, fixed)
}

/// The self-test's scenarios, synthetic, over the stand-in: every check of the integration's own
/// passes (no snap at the end of a rise, no pose jumps, no launch, no joint past its limit while
/// down, never under the ground under each body, no limb stretched, no light hit downing it, never
/// down again of its own, never a stiff body sliding, every rise with its hands planted, never the
/// game's ragdoll, the AI has it back, a corpse Delirium's).
#[test]
fn replay_the_selftest_scenarios_over_the_stand_in() {
    let (failed, fixed) = run_all(&physics_replay::synthetic());
    assert!(failed.is_empty(), "failed: {failed:#?}");
    assert!(fixed.is_empty(), "fixed, take off KNOWN_OPEN: {fixed:#?}");
}

/// A replay recorded by the game's self-test (`NV_PHYSICS_REPLAY`, a file; nothing to do without
/// one).
#[test]
fn replay_from_the_file() {
    let Some(path) = std::env::var_os("NV_PHYSICS_REPLAY") else {
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap();
    let scenarios = physics_replay::parse(&text).unwrap();
    let (failed, _) = run_all(&scenarios);
    assert!(failed.is_empty(), "failed: {failed:#?}");
}
