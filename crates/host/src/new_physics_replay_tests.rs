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

/// Open over Patrick's PC run on 5f52c0e (`replays/pc-5f52c0e.txt`), each reported to him:
/// - `downed-face-down`, down again of its own and the AI never has it again: acted as the
///   self-test's plan acts it (its light hit 1.5 s after it went down, as rising begins), New
///   Physics' rise sits up (the torso 25 to 29 degrees from upright, the pelvis 9 to 12 units up)
///   with a hand planted 25 units out carrying 350 to 500 N, never brings its feet under it, and
///   is given up and tried again. The planted-hands rise's step from sitting to crouching, not the
///   integration's; Patrick's PC run on 8a9b4a4's face down rise stands (its replay passes).
/// - `trench`, the AI has it again, the floor past 25 body weights and a rise with no hand
///   planted: that run's trench ground is only its bumps' height (15.8 units) laid as a sine every
///   100 units, 37-degree slopes everywhere (its ground grid as cast was not recorded yet); New
///   Physics' standing balance never comes calm on it, and rising, a hand put down meets no
///   ground near it. The ground as cast on Patrick's PC run on 8a9b4a4 (its grid) passes.
const KNOWN_OPEN_PC_5F52C0E: &[(&str, &str)] = &[
    ("downed-face-down", "the AI has it again"),
    ("downed-face-down", "never down again of its own"),
    ("trench", "the AI has it again"),
    ("trench", "no launch"),
    ("trench", "every rise with its hands planted"),
];

/// A recorded scenario acted by the self-test's plan of its name (`physics_selftest::PLANS`): which
/// of its acts is next, the wait under way, the hits made, and whether the kill is now.
struct PlanRun {
    acts: &'static [crate::physics_selftest::Act],
    next: usize,
    until: Option<f32>,
    hits: usize,
    from: usize,
    kill: bool,
}

impl PlanRun {
    /// The plan for `s` (none for a synthetic scenario, or one whose hits and kill are not the
    /// plan's: its frames as recorded).
    fn of(s: &Scenario) -> Option<PlanRun> {
        use crate::physics_selftest::Act;
        let plan = crate::physics_selftest::PLANS
            .iter()
            .find(|p| p.name == s.name)?;
        let hits = plan
            .acts
            .iter()
            .filter(|a| matches!(a, Act::Hit(..)))
            .count();
        let kills = plan.acts.iter().any(|a| matches!(a, Act::Kill));
        (hits == s.hits.len() && kills == s.kill.is_some() && !s.hits.is_empty()).then(|| PlanRun {
            acts: plan.acts,
            next: 0,
            until: None,
            hits: 0,
            from: s.hits[0].0,
            kill: false,
        })
    }

    /// Frame `i` (`t` s in; New Physics' watch last frame: down, rising): the recorded hits
    /// (their indices) the plan makes now; [`PlanRun::kill`] set if it kills now.
    fn step(&mut self, i: usize, t: f32, (down, rising): (bool, bool)) -> Vec<usize> {
        use crate::physics_selftest::Act;
        let mut made = Vec::new();
        self.kill = false;
        // From the frame its first hit was recorded at (the self-test's first acting frame).
        if i < self.from {
            return made;
        }
        while let Some(act) = self.acts.get(self.next) {
            let done = match *act {
                Act::Hit(..) => {
                    made.push(self.hits);
                    self.hits += 1;
                    true
                }
                Act::Wait(w) => {
                    let until = *self.until.get_or_insert(t + w);
                    t >= until - 1e-4
                }
                Act::UntilDown(most) => down || t >= most,
                Act::UntilRising(most) => rising || t >= most,
                Act::Kill => {
                    self.kill = true;
                    true
                }
            };
            if !done {
                break;
            }
            self.until = None;
            self.next += 1;
        }
        made
    }
}

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
    crate::gunplay::set_thread_tuning(Some(crate::gunplay::Tuning {
        new_physics: true,
        ..was
    }));
    // A human NPC (its base form an NPC's), with a ragdoll controller its corpse can be held by.
    f.e.mem.set_u32(ACTOR + 0x20, FAKE_BASE);
    f.e.mem.set_u8(FAKE_BASE + 4, 0x2a);
    f.e.mem.set_f32(FAKE_BASE + 0x1f8, 0.5);
    f.e.mem.set_u32(ACTOR + 0xac, FAKE_CONTROLLER);
    // The ground: the fixture's floor, tilted as the replay's, met by every cast.
    let at = f.reference();
    rig_fixture::SLOPE.with(|c| c.set(Some([at[0], at[1], s.slope[0], s.slope[1]])));
    // The ground as cast in the game (its grid) where the replay has it, else its bumps.
    let grid = s.grid.len() == 25;
    rig_fixture::GRID.with(|g| *g.borrow_mut() = grid.then(|| ([at[0], at[1]], s.grid.clone())));
    rig_fixture::BUMPS.with(|c| {
        c.set((s.bumps[0] > 0.0 && !grid).then_some([at[0], at[1], s.bumps[0], s.bumps[1]]));
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
    // A PC run's scenario is acted as the self-test's own plan acts it (its waits, until it is
    // down, until it rises), its hits as recorded: each hit and the kill land where they did in
    // the fall and the rise, whenever the fall and the rise come here (at the PC's frames, a hit
    // meant mid-rise came after this rise was over, a kill meant mid-rise killed it standing).
    let mut plan = PlanRun::of(s);
    let mut seen = (false, false);
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
        let hits_now: Vec<&(usize, String, f32, V3)> = match plan.as_mut() {
            Some(p) => p.step(i, t, seen).iter().map(|&k| &s.hits[k]).collect(),
            None => s.hits.iter().filter(|h| h.0 == i).collect(),
        };
        for (_, body, p, dir) in hits_now {
            let b = model().body(body).unwrap_or(model().root);
            let point = f.world_of(f.at[body_nodes[b]]).1;
            // As the game's self-test (and a bullet) does: New Physics takes the actor for the hit
            // (again, once it let it go to its animation), then the hit.
            assert!(
                claim(&mut f.e, ACTOR),
                "{}: not claimed for the hit at {t:.2} s",
                s.name
            );
            hit_shot(&mut f.e, ACTOR, *p, point, *dir, false);
            struck = struck.max(*p);
        }
        let kill_now = match plan.as_mut() {
            Some(p) => p.kill,
            None => s.kill == Some(i),
        };
        if kill_now && !died {
            died = true;
            f.e.mem.set_u32(ACTOR + 0x108, 1);
        }
        crate::active_ragdoll::apply(&mut f.e);
        crate::active_ragdoll::read_drawn(&mut f.e);
        let p = watch(ACTOR);
        seen = (
            p.as_ref().is_some_and(|p| p.down),
            p.as_ref().is_some_and(|p| p.down && p.rise),
        );
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
    // Nothing of this run left for the next test at the fixture's address (an actor New Physics
    // still had, its drawn pose written into the next fixture's nodes by its hook).
    crate::active_ragdoll::clear();
    rig_fixture::SLOPE.with(|c| c.set(None));
    rig_fixture::BUMPS.with(|c| c.set(None));
    rig_fixture::GRID.with(|g| *g.borrow_mut() = None);
    rig_fixture::WORLD.with(|w| *w.borrow_mut() = None);
    crate::gunplay::set_thread_tuning(None);
    let mut expect = s.expect();
    expect.standing = standing;
    let checks = physics_checks::judge(&frames, &expect, to_havok(ACTOR) - havok_before, None);
    physics_checks::lines(&s.name, &checks, false)
}

/// Every scenario of `scenarios` run, its lines printed; the failures not on `known` (as
/// [`KNOWN_OPEN`]: `scenario: check`), and those on it that passed.
fn run_all(scenarios: &[Scenario], known: &[(&str, &str)]) -> (Vec<String>, Vec<String>) {
    let (mut failed, mut fixed) = (Vec::new(), Vec::new());
    for s in scenarios {
        let (lines, _) = replay(s);
        for l in &lines {
            println!("{l}");
            let known = known
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
    let (failed, fixed) = run_all(&physics_replay::synthetic(), KNOWN_OPEN);
    assert!(failed.is_empty(), "failed: {failed:#?}");
    assert!(fixed.is_empty(), "fixed, take off KNOWN_OPEN: {fixed:#?}");
}

/// Patrick's PC run on 5f52c0e (the game's self-test with Delirium; `replays/pc-5f52c0e.txt`: its
/// hits, slopes, bumps and lengths), run again over the stand-in: what failed there for the
/// integration's own reasons (the refusals and retakes, the shoulders drawn off FNV's skeleton,
/// a rise given up standing, the corpse after the retakes) fails here too.
#[test]
fn replay_patricks_pc_run_on_5f52c0e() {
    let scenarios = physics_replay::parse(include_str!("replays/pc-5f52c0e.txt")).unwrap();
    assert_eq!(scenarios.len(), 11);
    let (failed, fixed) = run_all(&scenarios, KNOWN_OPEN_PC_5F52C0E);
    assert!(failed.is_empty(), "failed: {failed:#?}");
    assert!(fixed.is_empty(), "fixed, take off KNOWN_OPEN: {fixed:#?}");
}

/// Open over Patrick's PC run on 8a9b4a4 (`replays/pc-8a9b4a4.txt`), each reported to him: none.
const KNOWN_OPEN_PC_8A9B4A4: &[(&str, &str)] = &[];

/// Patrick's PC run on 8a9b4a4 (the game's self-test with Delirium; `replays/pc-8a9b4a4.txt`: its
/// hits, slopes, the trench's ground as cast under the body and its lengths), run again over the
/// stand-in, each scenario acted by its plan: what failed there for the integration's own reasons
/// (walls found in the floor under a lying body's feet and hands and step edges on a smooth bank,
/// each solid in Delirium's world; Delirium's own stagger downing it after a light hit; the first
/// rise given up while a hand was still on its way down) fails here too.
#[test]
fn replay_patricks_pc_run_on_8a9b4a4() {
    let scenarios = physics_replay::parse(include_str!("replays/pc-8a9b4a4.txt")).unwrap();
    assert_eq!(scenarios.len(), 11);
    let (failed, fixed) = run_all(&scenarios, KNOWN_OPEN_PC_8A9B4A4);
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
    let (failed, _) = run_all(&scenarios, KNOWN_OPEN);
    assert!(failed.is_empty(), "failed: {failed:#?}");
}
