//! Patrick's 18:39 and 18:41 clips on 231db2da (New Physics on): V.A.T.S.'s outline, highlights,
//! part labels and chances followed the animation's pose, not the body New Physics drew, for a
//! standing, a stepping and a downed NPC alike. The game's own V.A.T.S. here, in the game's frame
//! order: the animation's update (`Animation::UpdateMovement`, called at its address: the hook
//! `crate::knockdown::install` puts there runs the game's own and New Physics' snapshot after
//! it), the draw (New Physics' write, the world pass's read, the put-back), then the menu pass:
//! `VATSMenu::Create` (`007e9200`) once, `VATSMenu::DoIdle` (`007ec810`, which runs
//! `ShowTargetLimb` and `UpdateChanceToHit`) each frame, all called at their addresses, through
//! the frames of VATS's pause (frame time 0, the animation's update run on some and not others).
//! What VATS put in each part (`WorldCoord`, its chance, on screen) and what its limb highlight
//! (`00800ac0`'s effect target: the actor's 3D, whose skin the highlight draws) holds are read
//! back and set against the body drawn that frame (each skinned bone's world as the world pass
//! read it); a VATS shot at a part's `WorldCoord` goes through the game's capsule test on the
//! body nodes (`bhkCapsuleShape::GetClosestPoints` through `NI2HK`).
//!
//! The services outside V.A.T.S.'s reading of the target, answered by doubles (the fixture loads
//! no game data, no interface, no renderer, no sound): the menu's tiles, traits, fades and
//! texts; sounds; the HUD; the controls; the perks' entry points; the camera's projection (a
//! pinhole camera at the player's eye looking at the target); the body part data's parts and
//! node names (FNV's NPC body parts, each its skeleton node); line of sight (clear); the player's
//! weapon (a rifle's form) and the combat formula's skill (50).

// `!(x <= limit)`: a NaN fails the check too.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::*;
use crate::rig_fixture::{self, Fixture, Ground, ACTOR};

/// The test's own memory: the player, the menu, the body parts, the animation.
const AREA: u32 = 0x7700_0000;
const AREA_SIZE: u32 = 0x8_0000;
const PLAYER: u32 = AREA;
const PLAYER_VTABLE: u32 = AREA + 0x1000;
const PLAYER_PROCESS: u32 = AREA + 0x2000;
const PROCESS_VTABLE: u32 = AREA + 0x3000;
const ANIMATION: u32 = AREA + 0x4000;
const WEAPON_ITEM: u32 = AREA + 0x5000;
const WEAPON: u32 = AREA + 0x5100;
const CAMERA_HOLDER_AT: u32 = AREA + 0x6000;
const CAMERA_AT: u32 = AREA + 0x6400;
const TARGET_LIST: u32 = AREA + 0x7000;
const TARGET_ENTRY: u32 = AREA + 0x7100;
const CELL: u32 = AREA + 0x8000;
const NO_DESTRUCTION: u32 = AREA + 0x8800;
const NPC_BASE: u32 = AREA + 0x9000;
const NPC_BASE_VTABLE: u32 = AREA + 0xa000;
const BODY_PART_DATA: u32 = AREA + 0xb000;
const BODY_PARTS: u32 = AREA + 0xc000;
const MENU: u32 = AREA + 0x1_0000;
const MENU_VTABLE: u32 = AREA + 0x1_1000;
const MENU_TILE: u32 = AREA + 0x1_2000;
const TILES: u32 = AREA + 0x1_3000;
const HIGHLIGHT: u32 = AREA + 0x1_6000;
const PLAYER_ANIMATION: u32 = AREA + 0x1_7000;
const OWNER_VTABLE: u32 = AREA + 0x1_8000;
const VATS_LIGHT: u32 = AREA + 0x1_9000;
const BOUNDS: u32 = AREA + 0x1_a000;
const SCENE_NODE: u32 = AREA + 0x1_b000;
const PLAYER_ROOT: u32 = AREA + 0x1_c000;
const INVENTORY: u32 = AREA + 0x1_d000;
const AMMO: u32 = AREA + 0x1_d800;
const TILE_VTABLE: u32 = AREA + 0x1_e000;
const D3D_QUERY_VTABLE: u32 = AREA + 0x1_e800;
const SLOTS: u32 = AREA + 0x7_f000;
const ANSWER_NONE: u32 = SLOTS;
const ANSWER_YES: u32 = SLOTS + 4;
const ANSWER_SELF: u32 = SLOTS + 8;

/// The interface's clock (ms), advanced by each frame.
static CLOCK_MS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// VATS's paused frames run (2.5 s: past the menu's 2 s wait for the chances).
const VATS_FRAMES: u32 = 150;

/// The pose New Physics' actor is in as VATS opens.
#[derive(Clone, Copy, Debug)]
enum Pose {
    Standing,
    Stepping,
    Lying,
}

#[derive(Debug, Default)]
struct VatsRun {
    opened: bool,
    frames: u32,
    paused_frames: u32,
    paused_without_update: u32,
    /// The most a part's `WorldCoord` lay off its node drawn, and a highlight bone off the same
    /// bone drawn (units), and which.
    part_off: (f32, String),
    highlight_off: (f32, String),
    parts: usize,
    /// Visible parts whose chance is 0.
    no_chance: Vec<String>,
    /// The highlight's skin, head over feet (its outline's up), each VATS frame: the least.
    outline_up_least: f32,
    /// A VATS shot at each part: the bodies it met (or none), each part's own body.
    shots: Vec<(String, Vec<String>)>,
    down: bool,
    /// The last frame's drawn bones, and the most a shot's point lay outside its part's body
    /// drawn (units).
    last_drawn: Vec<(u32, (M3, V3))>,
    shot_off: f32,
    /// The highlight was on this actor in some VATS frame.
    highlighted: bool,
    /// New Physics had the actor as VATS opened.
    owned: bool,
}

fn vats_run(pose: Pose) -> VatsRun {
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
    f.game_update();
    lay_out(&mut f);
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    clear();
    set_keep_down(matches!(pose, Pose::Lying));
    let was = crate::gunplay::tuning();
    crate::gunplay::set_tuning(crate::gunplay::Tuning {
        new_physics: true,
        ..was
    });
    crate::knockdown::install(&mut f.e);
    let mut run = VatsRun {
        outline_up_least: f32::INFINITY,
        ..VatsRun::default()
    };
    let dt = 1.0 / 60.0;
    let mut t = 0.0f32;
    let hit_speed = match pose {
        Pose::Standing => 0.6,
        Pose::Stepping => 0.9,
        Pose::Lying => 4.35,
    };
    let vats_at = match pose {
        Pose::Standing => 0.4,
        Pose::Stepping => 0.45,
        Pose::Lying => 3.0,
    };
    // Until VATS opens: the game's frames.
    let mut claimed = false;
    while t < vats_at {
        t += dt;
        game_frame(&mut f, dt, true);
        if !claimed {
            claimed = claim(&mut f.e, ACTOR);
        }
        if (t - 0.2).abs() < dt / 2.0 {
            let spine = model().body("Bip01 Spine2").unwrap();
            let node = f.node(&model().bodies[spine].node).unwrap();
            let chest = f.world_of(node).1;
            hit(
                &mut f.e,
                ACTOR,
                hit_speed * model().mass,
                add(chest, [0.0, 8.0, 4.0]),
                [0.0, -1.0, 0.0],
            );
        }
    }
    run.down = is_down(ACTOR) && label_of(ACTOR) != Some("standing");
    run.owned = owns(ACTOR);
    // VATS: opened from the menu pass after a frame's draw, then its paused frames.
    let drawn_bones = game_frame(&mut f, dt, true);
    run.opened = f.e.call(crate::knockdown::VATS_CREATE, &[]).bool();
    // The player's VATS camera (a double here, [`project`]'s, already on the target) has reached
    // its heading: what the camera's turn sets in the menu (`bHeadingReached`, `+0xf8`).
    let menu = f.e.mem.u32(0x011d_b0d4);
    if menu != 0 {
        f.e.mem.set_u8(menu + 0xf8, 1);
    }
    measure(&mut f, &drawn_bones, &mut run);
    for i in 0..VATS_FRAMES {
        let update = i % 3 == 1;
        let drawn_bones = game_frame(&mut f, 0.0, update);
        run.paused_frames += 1;
        run.paused_without_update += u32::from(!update);
        let menu = f.e.mem.u32(0x011d_b0d4);
        // The interface manager's update: the highlight's effects (`00800370`), then the menu's.
        f.e.call(0x0080_0370, &[HIGHLIGHT]);
        f.e.call(crate::knockdown::VATS_DO_IDLE, &[menu]);
        measure(&mut f, &drawn_bones, &mut run);
        run.frames += 1;
    }
    shoot_parts(&mut f, &mut run);
    clear();
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    set_keep_down(false);
    crate::gunplay::set_tuning(was);
    run
}

/// One game frame: with time running the main thread's safe point; the animation's update for
/// the actor (when `update`) at its address; the draw (New Physics' write, the world pass's
/// read, the put-back). Answers each skinned bone's world as the world pass read it.
fn game_frame(f: &mut Fixture, dt: f32, update: bool) -> Vec<(u32, (M3, V3))> {
    f.e.mem.set_f32(FRAME_SECONDS, dt);
    CLOCK_MS.fetch_add(17, std::sync::atomic::Ordering::Relaxed);
    if dt > 0.0 {
        main_thread(&mut f.e);
    }
    if update {
        // The animation's frame begins (its "movement updated" bit cleared), the root where the
        // actor is.
        f.e.mem.set_u8(ANIMATION, 0);
        let reference = f.reference();
        rig_fixture::write3(
            &mut f.e,
            f.at[0] + rig_fixture::LOCAL_TRANSLATION,
            reference,
        );
        f.e.call(ANIMATION_UPDATE_MOVEMENT, &[ANIMATION, ACTOR]);
    }
    crate::active_ragdoll::apply(&mut f.e);
    let mut seen = Vec::new();
    let bones = skinned_bones(f);
    let limbs: Vec<(u32, u32, u32)> = PARTS
        .iter()
        .filter_map(|&(limb, name)| {
            let node = f.node(name)?;
            let i = f.at.iter().position(|&a| a == node)?;
            let child = (0..f.nodes.len())
                .find(|&c| f.nodes[c].parent == Some(i))
                .map_or(node, |c| f.at[c]);
            Some((limb, node, child))
        })
        .collect();
    let root = f.at[0];
    crate::active_ragdoll::draw_then_put_back(&mut f.e, |e| {
        crate::active_ragdoll::read_drawn(e);
        for &b in &bones {
            seen.push((
                b,
                (
                    read9(e, b + WORLD_ROTATION),
                    read3(e, b + WORLD_TRANSLATION),
                ),
            ));
        }
        render_highlight(e, root, &limbs);
    });
    seen
}

/// The renderer's VATS highlight pass (no renderer in the fixture: the GPU's part): for the
/// highlight's target (`00800ac0`'s actor 3D, `+8`), each limb without a result yet gets its two
/// occlusion queries (`+0x12c` all its pixels, `+0x16c` those on screen; `BSOcclusionQuery`, its
/// Direct3D query at `+0`), counted from the limb as drawn now: 16 points along its bone, from
/// its node to its child, through the camera ([`project`]), 100 pixels a point.
fn render_highlight(e: &mut Engine, root: u32, limbs: &[(u32, u32, u32)]) {
    if e.mem.u32(HIGHLIGHT + 8) != root {
        return;
    }
    for &(limb, node, child) in limbs {
        if limb >= 16
            || e.mem.u8(HIGHLIGHT + 0x1ac + limb) != 0
            || e.mem.u32(HIGHLIGHT + 0x16c + 4 * limb) != 0
        {
            continue;
        }
        let (a, b) = (
            read3(e, node + WORLD_TRANSLATION),
            read3(e, child + WORLD_TRANSLATION),
        );
        let mut seen = 0u32;
        for k in 0..16 {
            let t = k as f32 / 15.0;
            let p = add(a, scale(sub(b, a), t));
            let (x, y) = project(p);
            seen += u32::from((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
        }
        for (array, pixels) in [(0x12cu32, seen * 100), (0x16c, 1600)] {
            let query = e.mem.alloc(8);
            e.mem.set_u32(query, D3D_QUERY_VTABLE);
            e.mem.set_u32(query + 4, pixels);
            let occlusion = e.mem.alloc(8);
            e.mem.set_u32(occlusion, query);
            e.mem.set_u32(HIGHLIGHT + array + 4 * limb, occlusion);
        }
    }
}

/// The bones the actor's skinned geometry lists (what its draw and the VATS highlight skin
/// with).
fn skinned_bones(f: &Fixture) -> Vec<u32> {
    f.at.iter()
        .zip(&f.nodes)
        .filter(|(_, n)| n.name.starts_with("Bip01 "))
        .map(|(&a, _)| a)
        .collect()
}

/// After the menu pass: VATS's parts and highlight against the bones drawn.
fn measure(f: &mut Fixture, drawn: &[(u32, (M3, V3))], run: &mut VatsRun) {
    run.last_drawn = drawn.to_vec();
    let drawn_at = |node: u32| drawn.iter().find(|d| d.0 == node).map(|d| d.1 .1);
    // Each part VATS placed (`WorldCoord`) against its node drawn.
    let parts = vats_target_parts(&f.e, ACTOR);
    run.parts = run.parts.max(parts.len());
    let mut no_chance = Vec::new();
    for &(limb, at, chance, on_screen) in &parts {
        let Some(&(_, name)) = PARTS.iter().find(|p| p.0 == limb) else {
            continue;
        };
        let Some(node) = f.node(name) else { continue };
        if at != [0.0; 3] {
            if let Some(d) = drawn_at(node) {
                let off = length(sub(at, d));
                if off > run.part_off.0 {
                    run.part_off = (off, format!("limb {limb} ({name})"));
                }
            }
        }
        if on_screen && !(chance > 0.0) {
            no_chance.push(format!("limb {limb} ({name}): {chance}"));
        }
    }
    run.no_chance = no_chance;
    // The limb highlight (`00800ac0`'s target: this actor's 3D) and the bones its skin draws
    // with, as they are now (the menu's pass), against the same bones drawn.
    if f.e.mem.u32(HIGHLIGHT + 8) == f.at[0] {
        run.highlighted = true;
        for &b in &skinned_bones(f) {
            let now = read3(&f.e, b + WORLD_TRANSLATION);
            if let Some(d) = drawn_at(b) {
                let off = length(sub(now, d));
                if off > run.highlight_off.0 {
                    let i = f.at.iter().position(|&a| a == b).unwrap_or(0);
                    run.highlight_off = (off, f.nodes[i].name.clone());
                }
            }
        }
        // Its outline's up: the head over the lower foot.
        let z = |name: &str| f.node(name).map(|n| read3(&f.e, n + WORLD_TRANSLATION)[2]);
        if let (Some(head), Some(l), Some(r)) =
            (z("Bip01 Head"), z("Bip01 L Foot"), z("Bip01 R Foot"))
        {
            run.outline_up_least = run.outline_up_least.min(head - l.min(r));
        }
    }
}

/// A VATS shot at each part VATS placed: the game's capsule test (`bhkCapsuleShape::
/// GetClosestPoints`, `00cab3c0`, on each body's capsule at its node's world through `NI2HK`,
/// `0056df80`) at the part's `WorldCoord`, a sphere of 0.3 Havok units: the bodies it meets.
fn shoot_parts(f: &mut Fixture, run: &mut VatsRun) {
    // Its normalisation reads the exe's float constants 0, 3 and 0.5.
    for (at, v) in [
        (0x0101_1d78u32, 0.0f32),
        (0x0101_7718, 3.0),
        (0x0101_6248, 0.5),
    ] {
        f.e.mem.set_f32(at, v);
    }
    let area = f.e.mem.alloc(0x400);
    let body_nodes: Vec<u32> = model()
        .bodies
        .iter()
        .map(|b| f.node(&b.node).unwrap_or(0))
        .collect();
    for (limb, at, _, _) in vats_target_parts(&f.e, ACTOR) {
        let Some(&(_, name)) = PARTS.iter().find(|p| p.0 == limb) else {
            continue;
        };
        let mut met = Vec::new();
        for (b, def) in model().bodies.iter().enumerate() {
            let node = body_nodes[b];
            if node == 0 {
                continue;
            }
            let (wrapper, shape, sphere, xf, contact) =
                (area, area + 0x40, area + 0x80, area + 0xc0, area + 0x140);
            f.e.mem.set_u32(wrapper + 8, shape);
            let k = ragdoll_core::np_body::UNITS / rig_fixture::HK;
            f.e.mem.set_f32(shape + 0x10, def.radius * k);
            for (end, off) in def.ends.iter().zip([0x20u32, 0x30]) {
                for (j, x) in end.iter().enumerate() {
                    f.e.mem.set_f32(shape + off + 4 * j as u32, x * k);
                }
                f.e.mem.set_f32(shape + off + 12, 0.0);
            }
            for (j, x) in at.iter().enumerate() {
                f.e.mem.set_f32(sphere + 4 * j as u32, x / rig_fixture::HK);
            }
            f.e.mem.set_f32(sphere + 12, 0.3);
            f.e.call(0x0056_df80, &[xf, node + WORLD_ROTATION]);
            if f.e
                .call(0x00ca_b3c0, &[wrapper, sphere, xf, contact])
                .bool()
            {
                met.push(def.node.clone());
            }
        }
        // The shot's point against the part's body as drawn (its capsule at its node's world as
        // the last frame drew it): how far outside it.
        if let (Some(b), Some(node)) = (model().body(name), f.node(name)) {
            if let Some(&(_, (r, t))) = run.last_drawn.iter().find(|d| d.0 == node) {
                let def = &model().bodies[b];
                let k = ragdoll_core::np_body::UNITS;
                let ends = def
                    .ends
                    .map(|e| add(t, ragdoll_core::flinch::apply_m(&r, scale(e, k))));
                let d = sub(ends[1], ends[0]);
                let l2 = ragdoll_core::flinch::dot(d, d).max(1e-6);
                let u = (ragdoll_core::flinch::dot(sub(at, ends[0]), d) / l2).clamp(0.0, 1.0);
                let out = length(sub(at, add(ends[0], scale(d, u)))) - def.radius * k;
                run.shot_off = run.shot_off.max(out);
            }
        }
        run.shots.push((format!("limb {limb} ({name})"), met));
    }
}

/// The game's memory the path reads (see the module documentation).
fn lay_out(f: &mut Fixture) {
    let f_names: Vec<(u32, String)> =
        f.at.iter()
            .zip(&f.nodes)
            .map(|(&a, n)| (a, n.name.clone()))
            .collect();
    let root = f.at[0];
    let bip = f.node("Bip01").unwrap_or(f.at[1]);
    let e = &mut f.e;
    for page in (0x0100_0000u32..0x0128_0000).step_by(0x1000) {
        if !e.mem.is_mapped(page) {
            e.mem.map(page, 0x1000);
        }
    }
    e.mem.map(AREA, AREA_SIZE);
    e.register_double(ANSWER_NONE, |_, _| Ret::default());
    e.register_double(ANSWER_YES, |_, _| 1u32.into_ret());
    e.register_double(ANSWER_SELF, |_, a| a[0].into_ret());
    // The platform's thread slots and critical sections, its interlocked counts
    // (`InterlockedIncrement` `00fdf1c0`, `InterlockedDecrement` `00fdf1c4`) and its clock
    // (`GetTickCount` `00fdf060`, 16 ms a call).
    crate::av_table::tests::platform::install(e);
    e.register_double(0x00fd_f1c0, |e, a| {
        let v = e.mem.u32(a[0]).wrapping_add(1);
        e.mem.set_u32(a[0], v);
        v.into_ret()
    });
    e.register_double(0x00fd_f1c4, |e, a| {
        let v = e.mem.u32(a[0]).wrapping_sub(1);
        e.mem.set_u32(a[0], v);
        v.into_ret()
    });
    // The C runtime's `rand` (`00ecadb8`; its per-thread state is the runtime's start-up's): the
    // same generator on one state.
    let mut state = 1u32;
    e.register_double(0x00ec_adb8, move |_, _| {
        state = state.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        ((state >> 16) & 0x7fff).into_ret()
    });
    // `GetLastError` (`00fdf094`; the C runtime's per-thread data asks it).
    e.register_double(0x00fd_f094, |_, _| Ret::default());
    let mut tick = 1_000u32;
    e.register_double(0x00fd_f060, move |_, _| {
        tick += 16;
        tick.into_ret()
    });
    // The actor's unset slots answer nothing; its 3D (`+0x1d0`) the fixture's root. A node's
    // `+0x10` (not a fade node) nothing.
    let vt = e.mem.u32(ACTOR);
    for slot in 0..0x400u32 {
        if e.mem.u32(vt + 4 * slot) == 0 {
            e.mem.set_u32(vt + 4 * slot, ANSWER_NONE);
        }
    }
    const ROOT_OF: u32 = SLOTS + 12;
    e.register_double(ROOT_OF, move |_, _| root.into_ret());
    e.mem.set_u32(vt + 0x1d0, ROOT_OF);
    let nvt = e.mem.u32(root);
    for slot in 0..0x100u32 {
        if e.mem.u32(nvt + 4 * slot) == 0 {
            e.mem.set_u32(nvt + 4 * slot, ANSWER_NONE);
        }
    }
    // The animation (`Animation`): its root the actor's 3D, its accumulation root `Bip01`.
    e.mem.set_u32(ANIMATION + 0x08, root);
    e.mem.set_u32(ANIMATION + 0x0c, bip);
    // The fixture's process: its unset slots answer nothing.
    let pvt = e.mem.u32(rig_fixture::PROCESS_AT);
    for slot in 0..0x400u32 {
        if e.mem.u32(pvt + 4 * slot) == 0 {
            e.mem.set_u32(pvt + 4 * slot, ANSWER_NONE);
        }
    }
    // The player: in front of the actor, its slots answering nothing but its position; its
    // process holding a rifle.
    e.mem.set_u32(0x011d_ea3c, PLAYER);
    e.mem.set_u32(PLAYER, PLAYER_VTABLE);
    for slot in 0..0x400u32 {
        e.mem.set_u32(PLAYER_VTABLE + 4 * slot, ANSWER_NONE);
        e.mem.set_u32(PROCESS_VTABLE + 4 * slot, ANSWER_NONE);
    }
    write3(e, PLAYER + REFR_POSITION, PLAYER_AT);
    const POSITION_OF: u32 = SLOTS + 16;
    e.register_double(POSITION_OF, |_, a| (a[0] + REFR_POSITION).into_ret());
    e.mem.set_u32(PLAYER_VTABLE + 0x1f4, POSITION_OF);
    e.mem.set_u32(PLAYER_VTABLE + 0x100, ANSWER_YES);
    // Its 3D (`+0x1d0`): a node at its place.
    const PLAYER_3D: u32 = SLOTS + 48;
    e.register_double(PLAYER_3D, |_, _| PLAYER_ROOT.into_ret());
    e.mem.set_u32(PLAYER_VTABLE + 0x1d0, PLAYER_3D);
    write3(e, PLAYER_ROOT + WORLD_TRANSLATION, PLAYER_AT);
    // Its animation data (`+0x1e4`): no groups playing.
    const ANIMATION_OF: u32 = SLOTS + 32;
    e.register_double(ANIMATION_OF, |_, _| PLAYER_ANIMATION.into_ret());
    e.mem.set_u32(PLAYER_VTABLE + 0x1e4, ANIMATION_OF);
    e.mem.set_u32(PLAYER + 0x68, PLAYER_PROCESS);
    e.mem.set_u32(PLAYER + 0xa4, OWNER_VTABLE);
    e.mem.set_u32(PLAYER_PROCESS, PROCESS_VTABLE);
    const EQUIPPED: u32 = SLOTS + 20;
    e.register_double(EQUIPPED, |_, _| WEAPON_ITEM.into_ret());
    e.mem.set_u32(PROCESS_VTABLE + 0x148, EQUIPPED);
    e.mem.set_u32(WEAPON_ITEM + 8, WEAPON);
    // The rifle's form: a weapon (`TESForm` type `0x28` at `+4`), a two-handed rifle (`eType`
    // 5 at `+0xf4`); its other fields zero.
    e.mem.set_u8(WEAPON + 4, 0x28);
    e.mem.set_u8(WEAPON + 0xf4, 5);
    // Its current weapon (`008a1710`): the rifle.
    e.register_double(0x008a_1710, |_, _| WEAPON.into_ret());
    // The exe's constants the path reads (not loaded in the fixture).
    for (at, v) in [
        (0x0101_2070u32, 1.0f64),
        (0x0101_1590, 2.0),
        (0x0102_e430, 128.0),
        (0x0101_e568, 255.0),
        (0x0101_e2c0, 50.0),
    ] {
        e.mem.set_f64(at, v);
    }
    for (at, v) in [
        (0x0101_e704u32, 128.0f32),
        (0x0102_3cd8, 255.0),
        (0x0101_df44, 0.2),
        (0x0101_6248, 0.5),
        (0x0107_18c0, 1.0e-5),
        (0x0102_31d0, 150.0),
    ] {
        e.mem.set_f32(at, v);
    }
    e.mem.set_f64(0x0101_7a40, 100.0);
    e.mem.set_f64(0x0102_3128, std::f64::consts::PI / 180.0);
    e.mem.set_f64(0x0102_1928, 3.0);
    // The settings the path reads (a `Setting`'s value at `+4`; the fixture loads none, these
    // are the test's): VATS's engage distance, the sneak distances; the chance formula's
    // screen-percent factor 0.5, spread multiplier 1, the most a range's spread gives 1, its
    // hit-chance multiplier 1, its stealth multiplier 1 and the most chance 95.
    for (setting, v) in [
        (0x011d_45b8u32, 3000.0f32),
        (0x011c_dcbc, 3.0),
        (0x011c_d7d8, 1500.0),
        (0x011c_fab0, 0.5),
        (0x011c_fe08, 1.0),
        (0x011c_fe50, 1.0),
        (0x011c_f504, 1.0),
        (0x011c_ee54, 1.0),
        (0x011c_e10c, 95.0),
    ] {
        e.mem.set_f32(setting + 4, v);
    }
    // The camera (`CAMERA_HOLDER`, `CAMERA_OF`) and its projection (`NiCamera::WorldPtToScreenPt3`,
    // `00a6fc50`): a pinhole at the player's eye looking at the actor, 75 degrees across.
    e.register_double(0x0045_c670, |_, _| CAMERA_HOLDER_AT.into_ret());
    e.register_double(0x0066_29f0, |_, _| CAMERA_AT.into_ret());
    e.register_double(0x00a7_0ba0, |_, _| Ret::default());
    e.register_double(0x00a6_fc50, |e, a| {
        let p = read3(e, a[1]);
        let (x, y) = project(p);
        e.mem.set_f32(a[2], x);
        e.mem.set_f32(a[3], y);
        u32::from((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)).into_ret()
    });
    // The actors around the player (`00595d10`): the fixture's actor, hostile.
    e.mem.set_u32(TARGET_ENTRY, ACTOR);
    e.mem.set_u8(TARGET_ENTRY + 4, 1);
    e.mem.set_u32(TARGET_LIST, TARGET_ENTRY);
    e.mem.set_u32(TARGET_LIST + 4, 0);
    e.register_double(0x0059_5d10, |_, _| TARGET_LIST.into_ret());
    // The player's cell (`008d6f30`), an exterior (`00425fd0`), no world space (`00575d70`),
    // nothing to gather (`0054da20`, `005885f0`).
    e.register_double(0x008d_6f30, |_, _| CELL.into_ret());
    e.register_double(0x0042_5fd0, |_, _| Ret::default());
    e.register_double(0x0057_5d70, |_, _| Ret::default());
    e.register_double(0x0054_da20, |_, _| Ret::default());
    e.register_double(0x0058_85f0, |_, _| Ret::default());
    // No destruction data (`00475400`: a form with none).
    e.register_double(0x0047_5400, |_, _| NO_DESTRUCTION.into_ret());
    // The actor's process type (`00931850`): high.
    e.register_double(0x0093_1850, |_, _| Ret::default());
    // The distance between two references (`005723b0`): between their positions.
    e.register_double(0x0057_23b0, |e, a| Ret {
        st0: length(sub(
            read3(e, a[0] + REFR_POSITION),
            read3(e, a[1] + REFR_POSITION),
        )) as f64,
        ..Ret::default()
    });
    // The combat utilities' distance between two facing actors (`009a64d0`: less their
    // extents, which need their processes' cached sizes) and a weapon's reach (`009a69c0`): the
    // distance between their places; no reach (a rifle).
    e.register_double(0x009a_64d0, |e, a| Ret {
        st0: length(sub(
            read3(e, a[0] + REFR_POSITION),
            read3(e, a[1] + REFR_POSITION),
        )) as f64,
        ..Ret::default()
    });
    e.register_double(0x009a_69c0, |_, _| Ret::default());
    // Line of sight (`0088b880`): clear.
    e.register_double(0x0088_b880, |_, _| 1u32.into_ret());
    // The actor's actor values (`+0xa4`, an owner whose slots answer them): each 100.
    e.mem.set_u32(ACTOR + 0xa4, OWNER_VTABLE);
    const HUNDRED: u32 = SLOTS + 36;
    e.register_double(HUNDRED, |_, _| Ret {
        eax: 100,
        st0: 100.0,
        ..Ret::default()
    });
    for slot in 0..0x40u32 {
        e.mem.set_u32(OWNER_VTABLE + 4 * slot, HUNDRED);
    }
    // The actor's base form and its body part data (`BGSBodyPartData`, slot `0x180`): FNV's NPC
    // parts, each its skeleton node (a `BGSBodyPart`'s node name at `+0xc`, what `0043b230`
    // reads), in the data's part slots (`+0x34`).
    e.mem.set_u32(ACTOR + 0x20, NPC_BASE);
    e.mem.set_u32(NPC_BASE, NPC_BASE_VTABLE);
    // Its part at `+0x30` (its slots answer nothing either).
    e.mem.set_u32(NPC_BASE + 0x30, NPC_BASE_VTABLE);
    for slot in 0..0x400u32 {
        e.mem.set_u32(NPC_BASE_VTABLE + 4 * slot, ANSWER_NONE);
    }
    const BODY_PART_DATA_OF: u32 = SLOTS + 24;
    e.register_double(BODY_PART_DATA_OF, |_, _| BODY_PART_DATA.into_ret());
    e.mem.set_u32(NPC_BASE_VTABLE + 0x180, BODY_PART_DATA_OF);
    for (k, &(limb, node)) in PARTS.iter().enumerate() {
        let part = BODY_PARTS + 0x100 * k as u32;
        let name = part + 0x80;
        e.mem.set_cstr(name, node.as_bytes());
        e.mem.set_u32(part + 0xc, name);
        // Its to-hit chance (`+0x64`, a byte, percent): the head 60, the rest 90.
        e.mem
            .set_u8(part + 0x64, if node == "Bip01 Head" { 60 } else { 90 });
        e.mem.set_u32(BODY_PART_DATA + 0x34 + 4 * limb, part);
    }
    // A node found by name under a node (`004aae30`, the scene graph's lookup): the fixture's
    // tree searched for the name.
    let tree: Vec<(u32, String)> = f_names.clone();
    e.register_double(0x004a_ae30, move |e, a| {
        let want = String::from_utf8_lossy(&e.mem.cstr(a[1])).into_owned();
        let mut found = 0u32;
        for (node, name) in &tree {
            if *name == want && under(e, *node, a[0]) {
                found = *node;
                break;
            }
        }
        found.into_ret()
    });
    // The menu's interface (its tiles, traits, fades, texts; the HUD; sounds; the controls): no
    // menu but V.A.T.S.'s open (`00a09030`: none of a class; `00702450`: V.A.T.S.'s, 0x420, on
    // top), its file read (`00a01b00`) the menu's tile, whose menu (`00a03c90`) is the V.A.T.S.
    // menu block, of its class (slot `0x34`); the rest of the interface answers nothing.
    e.register_double(0x00a0_9030, |_, _| Ret::default());
    // The C runtime's `fmod` (`00ec9130`) and `atan` (`00ec9010`; their dispatch tables are the
    // exe's data, not loaded): the remainder and the arc tangent, as C has them. And the exe's constants the HUD's heading reads: pi, pi / 2,
    // degrees per radian.
    e.register_double(0x00ec_9130, |_, a| {
        let y = f64::from_bits(u64::from(a[0]) | (u64::from(a[1]) << 32));
        let x = f64::from_bits(u64::from(a[2]) | (u64::from(a[3]) << 32));
        Ret {
            st0: y % x,
            ..Ret::default()
        }
    });
    e.register_double(0x00ec_9010, |_, a| Ret {
        st0: f64::from_bits(u64::from(a[0]) | (u64::from(a[1]) << 32)).atan(),
        ..Ret::default()
    });
    e.mem.set_f64(0x0101_ff40, std::f64::consts::PI);
    e.mem.set_f64(0x0101_ff48, std::f64::consts::TAU);
    e.mem.set_f32(0x0101_ff50, std::f32::consts::TAU);
    e.mem.set_f64(0x0101_ff58, -std::f64::consts::PI);
    e.mem.set_f32(0x0102_b3c8, std::f32::consts::FRAC_PI_2);
    e.mem.set_f64(0x0102_f248, 180.0 / std::f64::consts::PI);
    // The game's heap (`operator new` `00401000`, Gamebryo's `00aa1070`): blocks of the
    // engine's own (the fixture sets up no heap). Then the start-up step that builds the fast
    // square root's table (`00a7e930`, kept at `011f4aa8`), as the game's start does.
    e.register_double(0x0040_1000, |e, a| e.mem.alloc(a[0].max(4)).into_ret());
    e.register_double(0x00aa_1070, |e, a| e.mem.alloc(a[0].max(4)).into_ret());
    e.call(0x00a7_e930, &[]);
    // The C runtime's `vsprintf_s` (`00ec6bb5`; its locale is the runtime's start-up's, not run
    // in the fixture): `%i`, `%d`, `%u`, `%s` and `%%`, what the menu's texts use.
    e.register_double(0x00ec_6bb5, |e, a| {
        let format = if a[2] == 0 {
            Vec::new()
        } else {
            e.mem.cstr(a[2])
        };
        let mut out = String::new();
        let mut arg = a[3];
        let mut chars = format.iter().copied();
        while let Some(c) = chars.next() {
            if c != b'%' {
                out.push(c as char);
                continue;
            }
            match chars.next() {
                Some(b'%') => out.push('%'),
                Some(b'i' | b'd') => {
                    out.push_str(&e.mem.i32(arg).to_string());
                    arg += 4;
                }
                Some(b'u') => {
                    out.push_str(&e.mem.u32(arg).to_string());
                    arg += 4;
                }
                Some(b's') => {
                    let text = e.mem.u32(arg);
                    if text != 0 {
                        out.push_str(&String::from_utf8_lossy(&e.mem.cstr(text)));
                    }
                    arg += 4;
                }
                _ => {}
            }
        }
        let size = a[1].max(1) as usize;
        let bytes: Vec<u8> = out.bytes().take(size - 1).collect();
        e.mem.set_cstr(a[0], &bytes);
        (bytes.len() as u32).into_ret()
    });
    // A reference's name for the menu (`0055d520`; the base form's full name, the fixture's
    // forms have no names): "NPC".
    e.mem.set_cstr(SLOTS + 0xa40, b"NPC");
    e.register_double(0x0055_d520, |_, _| (SLOTS + 0xa40).into_ret());
    // The game's language (`sLanguage`, `011c3cb4`) and the exe's "ENGLISH" (`01018348`).
    e.mem.set_cstr(SLOTS + 0xa00, b"ENGLISH");
    e.mem.set_u32(0x011c_3cb4 + 4, SLOTS + 0xa00);
    e.mem.set_cstr(0x0101_8348, b"ENGLISH");
    // The menus' folder (`004b7210`, `00586150`: the path the menu's file is read from).
    e.register_double(0x004b_7210, |_, _| Ret::default());
    e.mem.set_cstr(SLOTS + 0x900, b"Data\\Menus\\");
    e.register_double(0x0058_6150, |_, _| (SLOTS + 0x900).into_ret());
    e.register_double(0x0070_2450, |_, a| u32::from(a[0] == 0x420).into_ret());
    e.register_double(0x00a0_1b00, |_, _| MENU_TILE.into_ret());
    e.register_double(0x00a0_3c90, |_, a| {
        if a[0] == MENU_TILE { MENU } else { 0 }.into_ret()
    });
    // `__RTDynamicCast` (`00ec43fb`): the menu's tile and object are what the menu's casts want
    // (the fixture's blocks have no RTTI); every other cast the C runtime's own.
    e.register_double(0x00ec_43fb, |e, a| {
        if a[0] == MENU || a[0] == MENU_TILE {
            a[0].into_ret()
        } else {
            engine_core::units::crt::rtti::rt_dynamic_cast(e, a[0], a[1], a[2], a[3], a[4])
                .into_ret()
        }
    });
    const MENU_CLASS: u32 = SLOTS + 28;
    e.register_double(MENU_CLASS, |_, _| 0x420u32.into_ret());
    e.mem.set_u32(MENU, MENU_VTABLE);
    for slot in 0..0x100u32 {
        e.mem.set_u32(MENU_VTABLE + 4 * slot, ANSWER_NONE);
    }
    e.mem.set_u32(MENU_VTABLE + 0x34, MENU_CLASS);
    for i in 0..32u32 {
        e.mem.set_u32(MENU + 0x28 + 4 * i, TILES + 0x100 * i);
    }
    for at in INTERFACE {
        e.register_double(at, |_, _| Ret::default());
    }
    // A tile found by its path (`00a08b20`) or made from a template (`00a1ddb0`): a tile of its
    // own each time.
    fn new_tile(e: &mut Engine) -> u32 {
        let tile = e.mem.alloc(0x100);
        e.mem.set_u32(tile, TILE_VTABLE);
        tile
    }
    e.register_double(0x00a0_8b20, |e, _| new_tile(e).into_ret());
    e.register_double(0x00a1_ddb0, |e, _| new_tile(e).into_ret());
    for slot in 0..0x100u32 {
        e.mem.set_u32(TILE_VTABLE + 4 * slot, ANSWER_NONE);
    }
    e.mem.set_u32(MENU_TILE, TILE_VTABLE);
    for i in 0..32u32 {
        e.mem.set_u32(TILES + 0x100 * i, TILE_VTABLE);
    }
    // The renderer's compass texture (`00a5fd70`) and the string pool's names (`00438170`, made
    // from a text: the text itself; `004381b0` released): a texture block, the texts as they
    // are.
    e.register_double(0x00a5_fd70, |e, _| {
        let texture = e.mem.alloc(0x40);
        e.mem.set_u32(texture, TILE_VTABLE);
        texture.into_ret()
    });
    e.register_double(0x0043_8170, |e, a| {
        e.mem.set_u32(a[0], a[1]);
        a[0].into_ret()
    });
    e.register_double(0x0043_81b0, |_, _| Ret::default());
    // Sounds (`0041a250`, `00418900`: a handle made and assigned) and the player's iron sights
    // (`008bb650`).
    e.register_double(0x0041_a250, |_, a| a[0].into_ret());
    e.register_double(0x0041_8900, |_, a| a[0].into_ret());
    e.register_double(0x008b_b650, |_, _| Ret::default());
    // The interface's clock in milliseconds (`00825c00`): the frames' (17 ms each, also through
    // VATS's pause: the clock is real time).
    CLOCK_MS.store(10_000, std::sync::atomic::Ordering::Relaxed);
    e.register_double(0x0082_5c00, |_, _| {
        CLOCK_MS
            .load(std::sync::atomic::Ordering::Relaxed)
            .into_ret()
    });
    // A tile's first child (`0073ac90`): none (a word of 0).
    e.register_double(0x0073_ac90, |_, _| (SLOTS + 0x800).into_ret());
    // The combat formulas' damage of a VATS command (`009b4fe0`, the probable damage VATS shows)
    // and of the command's hit data (`009b5050`): none (the player's skills, perks and weapon
    // stats are not in the fixture; the chances do not read them).
    e.register_double(0x009b_4fe0, |_, _| Ret::default());
    e.register_double(0x009b_5050, |_, _| Ret::default());
    // VATS's light (the `VATS` object's `+0x34`), made the first time VATS ran (a renderer
    // object: a block of its own here).
    e.mem.set_u32(0x011f_2250 + 0x34, VATS_LIGHT);
    e.mem.set_u32(VATS_LIGHT + 0xf8, VATS_LIGHT + 0x400);
    // Its light put on the target (`009c9ae0`: the light's place, colour and dimmer, traced
    // through the scene): the renderer's, nothing here.
    e.register_double(0x009c_9ae0, |_, _| Ret::default());
    // The player's inventory (`004bf220`, `004c8f30`: 30 of anything asked), its weapon's ammo
    // (`00525980`), clip (`004fe160`: 30) and mods (`004bda70`: none).
    e.register_double(0x004b_f220, |_, _| INVENTORY.into_ret());
    e.register_double(0x004c_8f30, |_, _| 30u32.into_ret());
    e.register_double(0x0052_5980, |_, _| AMMO.into_ret());
    e.register_double(0x004f_e160, |_, _| 30u32.into_ret());
    e.register_double(0x004b_da70, |_, _| Ret::default());
    // The rifle's spread (`00524b80`, its form's: 1 degree) and the tangent the chance takes of
    // it (`0057dd70`; the C runtime's `tan`, its dispatch the exe's data).
    e.register_double(0x0052_4b80, |_, _| Ret {
        st0: 1.0,
        ..Ret::default()
    });
    e.register_double(0x0057_dd70, |_, a| Ret {
        st0: f64::from(f32::from_bits(a[0]).tan()),
        ..Ret::default()
    });
    // The rifle's health (`004873d0`, the form's; `004bcdb0`, the item's): 100, in full.
    e.register_double(0x0048_73d0, |_, _| 100u32.into_ret());
    e.register_double(0x004b_cdb0, |_, _| Ret {
        st0: 100.0,
        ..Ret::default()
    });
    // The player's VATS camera (`009445b0`: the first-person camera turned to the target): the
    // fixture's camera is the pinhole of [`project`], looking at the target already.
    e.register_double(0x0094_45b0, |_, _| Ret::default());
    // The player's camera's update and its first-person zoom (`0094ae40`, `00950290`): the same
    // camera.
    e.register_double(0x0094_ae40, |_, _| Ret::default());
    e.register_double(0x0095_0290, |_, _| Ret::default());
    // The player's auto-aim pick (`00964260`, a ray through the crosshair): not VATS's.
    e.register_double(0x0096_4260, |_, _| Ret::default());
    // The player's own update in VATS's frame (`008d3550`, its animation's `008885e0`): the
    // player is the camera here (no body, no animation).
    e.register_double(0x008d_3550, |_, _| Ret::default());
    e.register_double(0x0088_85e0, |_, _| Ret::default());
    // The renderer's scene (`00450b80`, its shadow scene node) and its light trace (`00b5b610`,
    // how much of a light reaches a point): a node of no lights, the light unblocked.
    e.register_double(0x0045_0b80, |_, _| SCENE_NODE.into_ret());
    e.register_double(0x00b5_b610, |_, _| Ret::default());
    // The actor's bounds (`+0x1d8` the lower corner, `+0x1dc` the upper): an NPC's, 128 tall.
    write3(e, BOUNDS, [-20.0, -20.0, 0.0]);
    write3(e, BOUNDS + 0x10, [20.0, 20.0, 128.0]);
    const LOWER: u32 = SLOTS + 40;
    const UPPER: u32 = SLOTS + 44;
    e.register_double(LOWER, |_, _| BOUNDS.into_ret());
    e.register_double(UPPER, |_, _| (BOUNDS + 0x10).into_ret());
    e.mem.set_u32(vt + 0x1d8, LOWER);
    e.mem.set_u32(vt + 0x1dc, UPPER);
    // A Direct3D occlusion query (the highlight pass's, [`render_highlight`]): `GetData`
    // (`+0x1c`) gives its pixel count at once (S_OK); `Release` and the rest answer nothing.
    for slot in 0..0x20u32 {
        e.mem.set_u32(D3D_QUERY_VTABLE + 4 * slot, ANSWER_NONE);
    }
    const GET_DATA: u32 = SLOTS + 52;
    e.register_double(GET_DATA, |e, a| {
        let pixels = e.mem.u32(a[0] + 4);
        e.mem.set_u32(a[1], pixels);
        Ret::default()
    });
    e.mem.set_u32(D3D_QUERY_VTABLE + 0x1c, GET_DATA);
    // `operator delete` (`00401030`): the engine's blocks of the doubled `operator new`.
    e.register_double(0x0040_1030, |_, _| Ret::default());
    // The highlight's flicker interval (an integer game setting at `011db75c`, ms; the fixture
    // loads no settings): 2 s.
    e.mem.set_u32(0x011d_b75c + 4, 2000);
    // The render target's scale of the highlight's counts (`+0x2c8`): 1.
    e.mem.set_u32(HIGHLIGHT + 0x2c8, 1);
    // The interface manager's limb highlight (`00705910`): the effect `00800ac0` targets.
    e.register_double(0x0070_5910, |_, _| HIGHLIGHT.into_ret());
}

/// Whether `node` lies under `root` (or is it).
fn under(e: &Engine, mut node: u32, root: u32) -> bool {
    for _ in 0..64 {
        if node == root {
            return true;
        }
        if node == 0 {
            return false;
        }
        node = e.mem.u32(node + rig_fixture::PARENT);
    }
    false
}

/// FNV's NPC body parts (`BGSBodyPart::LIMB_ENUM`) and their skeleton nodes.
const PARTS: [(u32, &str); 13] = [
    (0, "Bip01 Spine2"),
    (1, "Bip01 Head"),
    (3, "Bip01 L UpperArm"),
    (4, "Bip01 L Forearm"),
    (5, "Bip01 R UpperArm"),
    (6, "Bip01 R Forearm"),
    (7, "Bip01 L Thigh"),
    (8, "Bip01 L Calf"),
    (9, "Bip01 L Foot"),
    (10, "Bip01 R Thigh"),
    (11, "Bip01 R Calf"),
    (12, "Bip01 R Foot"),
    (13, "Bip01 Head"),
];

/// The interface's functions V.A.T.S.'s menu calls for its tiles, traits, fades, texts, the
/// HUD's meters and compass, sounds and the controls (`vatsmenu.rs`'s constants of those).
const INTERFACE: [u32; 68] = [
    0x0070_0320,
    0x0070_37e0,
    0x0070_3d50,
    0x0070_5780,
    0x0070_9430,
    0x0070_9d40,
    0x0070_bc20,
    0x0070_ec70,
    0x0071_5d40,
    0x0071_5da0,
    0x0071_77c0,
    0x0071_7820,
    0x0071_7a40,
    0x0071_8630,
    0x0072_42c0,
    0x0072_6090,
    0x0072_6530,
    0x0072_69d0,
    0x0072_6b40,
    0x0073_b020,
    0x0074_7d00,
    0x0077_00b0,
    0x0077_00d0,
    0x0077_00f0,
    0x0077_4800,
    0x0077_48b0,
    0x0077_4b00,
    0x0077_5380,
    0x0077_9070,
    0x00a0_0940,
    0x00a0_0e90,
    0x00a0_0f30,
    0x00a0_11b0,
    0x00a0_11f0,
    0x00a0_1230,
    0x00a0_12d0,
    0x00a0_1350,
    0x00a0_1530,
    0x00a0_1860,
    0x00a0_3da0,
    0x00a0_40a0,
    0x00a0_4200,
    0x00a0_7c60,
    0x00a0_7dc0,
    0x00a0_7f30,
    0x00a0_7fc0,
    0x00a0_8070,
    0x00a0_8b20,
    0x00a0_9080,
    0x00a0_9130,
    0x00a0_9200,
    0x00a0_9410,
    0x00a1_d910,
    0x00a1_dc20,
    0x00a1_dc70,
    0x00a1_ddb0,
    0x00a2_3390,
    0x00a2_3a50,
    0x00a2_4180,
    0x00a2_4660,
    0x00a2_4f10,
    0x00ad_7550,
    0x00ad_8510,
    0x00ad_8540,
    0x00ad_8780,
    0x00ad_8830,
    0x00ad_9060,
    0x00ae_5680,
];

/// The player's place (game units): in front of the actor, 250 units off; its eye 120 up.
const PLAYER_AT: V3 = [0.0, 250.0, 0.0];
const EYE: V3 = [0.0, 250.0, 120.0];
const LOOK_AT: V3 = [0.0, -40.0, 40.0];

/// `p` on the screen (0 to 1 across and up) of the pinhole camera at [`EYE`] looking at
/// [`LOOK_AT`], 75 degrees across, 16:9; behind it, -1.
fn project(p: V3) -> (f32, f32) {
    let f = ragdoll_core::flinch::unit(sub(LOOK_AT, EYE)).unwrap_or([0.0, -1.0, 0.0]);
    let r = ragdoll_core::flinch::unit(ragdoll_core::flinch::cross(f, [0.0, 0.0, 1.0]))
        .unwrap_or([1.0, 0.0, 0.0]);
    let u = ragdoll_core::flinch::cross(r, f);
    let d = sub(p, EYE);
    let z = ragdoll_core::flinch::dot(d, f);
    if z <= 1.0 {
        return (-1.0, -1.0);
    }
    let half = (37.5f32).to_radians().tan();
    let x = 0.5 + 0.5 * ragdoll_core::flinch::dot(d, r) / (z * half);
    let y = 0.5 + 0.5 * ragdoll_core::flinch::dot(d, u) / (z * half * 9.0 / 16.0);
    (x, y)
}

/// Patrick's 18:39 and 18:41 clips: through the game's VATS (`VATSMenu::Create`, then
/// `DoIdle` each paused frame, the animation's update on some of them and not others), for New
/// Physics' actor standing after a light hit, stepping from a harder one and lying after a fall:
/// every part VATS placed lies within 2 units of its node drawn, every bone the limb highlight
/// skins with within 2 units of the same bone drawn, the lying body's outline not upright (its
/// head less than 40 units over its lower foot), every part on screen given a chance above 0, and
/// a VATS shot at each part meets that part's own body in the game's capsule test.
#[test]
fn np_vats_reads_the_body_drawn() {
    let mut failed = Vec::new();
    for pose in [Pose::Standing, Pose::Stepping, Pose::Lying] {
        let r = vats_run(pose);
        eprintln!(
            "RESULT np vats {pose:?}: opened {}, owned {}, down {}, {} parts, {} paused frames ({} \
             with no animation update), the parts at most {:.2} u off the body drawn ({}), the \
             highlight's bones {:.2} u ({}), the outline's up {:.1} u, no chance {:?}, the shots' \
             points at most {:.2} u outside their parts drawn, shots {:?}",
            r.opened,
            r.owned,
            r.down,
            r.parts,
            r.paused_frames,
            r.paused_without_update,
            r.part_off.0,
            r.part_off.1,
            r.highlight_off.0,
            r.highlight_off.1,
            r.outline_up_least,
            r.no_chance,
            r.shot_off,
            r.shots
        );
        let mut fail = |what: String| failed.push(format!("{pose:?}: {what}"));
        if !r.opened || !r.owned || r.parts == 0 || !r.highlighted {
            fail(format!(
                "opened {}, owned {}, {} parts, highlighted {}",
                r.opened, r.owned, r.parts, r.highlighted
            ));
        }
        if r.paused_without_update == 0 {
            fail("no paused frame without the animation's update".to_string());
        }
        if !(r.part_off.0 <= 2.0) {
            fail(format!(
                "a part {:.2} u off ({})",
                r.part_off.0, r.part_off.1
            ));
        }
        if !(r.highlight_off.0 <= 2.0) {
            fail(format!(
                "a highlight bone {:.2} u off ({})",
                r.highlight_off.0, r.highlight_off.1
            ));
        }
        if !(r.shot_off <= 0.5) {
            fail(format!(
                "a shot's point {:.2} u outside its part drawn",
                r.shot_off
            ));
        }
        if !r.no_chance.is_empty() {
            fail(format!("no chance: {:?}", r.no_chance));
        }
        for (part, met) in &r.shots {
            let own = PARTS
                .iter()
                .find(|p| part.contains(&format!("({})", p.1)))
                .map(|p| p.1);
            if !own.is_some_and(|o| met.iter().any(|m| m == o)) {
                fail(format!("a shot at {part} met {met:?}"));
            }
        }
        if matches!(pose, Pose::Lying) {
            if !r.down {
                fail("not lying".to_string());
            }
            if !(r.outline_up_least < 40.0) {
                fail(format!("the outline upright: {:.1} u", r.outline_up_least));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}
