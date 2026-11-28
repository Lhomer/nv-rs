//! Tests only: an actor's 3D built in the engine's memory from a node tree (each node's name,
//! parent, local rotation, translation and scale, and whether a ragdoll body hangs from it), with
//! the game's calls the reactions (`crate::active_ragdoll`) and the knockdowns
//! (`crate::knockdown`) make answered by doubles over it: the node walk, the update, the actor's
//! form and process, the ragdoll's bodies and their velocities, the character controller, the
//! navmesh. The trees come from a skeleton's dump (FNV's `skeleton.nif`, from the game's files on
//! a machine that has them: game data, never in git) or, for the tests that need none, the lab's
//! mannequin laid out as FNV's tree is.

use engine_core::prelude::*;
use ragdoll_core::flinch::{add, apply_m, length, mul, scale, sub, transpose, IDENTITY, M3, V3};

/// A node of the tree: its name, parent (an index, earlier in the list), local transform, and
/// whether a ragdoll body hangs from it.
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub r: M3,
    pub t: V3,
    pub s: f32,
    pub body: bool,
}

/// The node's layout (`NiAVObject`): name, parent, local rotation, translation, scale, world
/// rotation, translation and scale.
pub const NODE_NAME: u32 = 0x08;
pub const PARENT: u32 = 0x18;
pub const LOCAL_ROTATION: u32 = 0x34;
pub const LOCAL_TRANSLATION: u32 = 0x58;
pub const LOCAL_SCALE: u32 = 0x64;
pub const WORLD_ROTATION: u32 = 0x68;
pub const WORLD_TRANSLATION: u32 = 0x8c;
pub const WORLD_SCALE: u32 = 0x98;

/// A tree for the per-tree doubles: its actor, form id, nodes' addresses and nodes' parents.
type Tree = (u32, u32, Vec<u32>, Vec<Option<usize>>);

/// The game's calls answered here.
const ACTOR_3D: u32 = 0x0043_fcd0;
const CHILD_COUNT: u32 = 0x0043_b480;
const CHILD_AT: u32 = 0x0043_b4a0;
const UPDATE_DATA: u32 = 0x0043_d410;
const NODE_UPDATE: u32 = 0x00a5_9c60;
const LOOKUP_FORM: u32 = 0x0048_39c0;
const GET_SCALE: u32 = 0x0056_7400;
const IS_WEAPON_DRAWN: u32 = 0x008a_16d0;
const NODE_COLLISION_OBJECT: u32 = 0x0043_b610;
const COLLISION_RIGID_BODY: u32 = 0x006f_a820;
const HAVOK_BODY: u32 = 0x0062_0b80;
const LINEAR_VELOCITY: u32 = 0x0056_0dc0;
const ANGULAR_VELOCITY: u32 = 0x0056_0e50;
const SET_LINEAR_VELOCITY: u32 = 0x0056_16d0;
const SET_ANGULAR_VELOCITY: u32 = 0x0056_1800;
const BODY_SET_POSITION_AND_ROTATION: u32 = 0x00ca_8960;
/// A body's position and rotation quaternion (`crate::knockdown`'s `BODY_TRANSLATION`,
/// `BODY_ROTATION`).
pub const BODY_TRANSLATION: u32 = 0x120;
pub const BODY_ROTATION: u32 = 0x160;
const SET_LOCATION: u32 = 0x0057_5830;
const CHAR_CONTROLLER: u32 = 0x0093_06d0;
const CONTROLLER_SET_POSITION: u32 = 0x0056_20e0;
const CONTROLLER_SET_VELOCITY: u32 = 0x0092_6980;
const CONTROLLER_VELOCITY: u32 = 0x0080_7080;
const PATHING_LOCATION_AT: u32 = 0x006d_ce10;
const PATHING_POSITION: u32 = 0x04;
const RESOLVE_TO_NAVMESH: u32 = 0x006d_dc00;
const IS_VALID_LOCATION: u32 = 0x006d_ed70;
/// Game units per Havok unit.
const HK_TO_GAME: u32 = 0x011b_1864;
const GAME_TO_HK: u32 = 0x011c_582c;
pub const HK: f32 = 7.0;

/// The actor's record and its process (`crate::active_ragdoll`'s offsets).
pub const FORM_ID: u32 = 0x0c;
pub const REFR_POSITION: u32 = 0x30;
pub const PROCESS: u32 = 0x68;
pub const KNOCK_STATE: u32 = 0x13c;
/// A body's motion type (`crate::knockdown`'s `MOTION_TYPE`).
const MOTION_TYPE: u32 = 0xe8;

/// The layout of the fixture's memory.
const BASE: u32 = 0x0200_0000;
const SIZE: u32 = 0x0010_0000;
const NODE_SIZE: u32 = 0x100;
const NAMES: u32 = BASE + 0x8_0000;
const BODIES: u32 = BASE + 0xa_0000;
const BODY_SIZE: u32 = 0x200;
pub const ACTOR: u32 = BASE + 0xc_0000;
pub const PROCESS_AT: u32 = BASE + 0xc_1000;
pub const CONTROLLER: u32 = BASE + 0xc_2000;
const NODE_VTABLE: u32 = BASE + 0xc_3000;
const PROCESS_VTABLE: u32 = BASE + 0xc_4000;
const PATHING: u32 = BASE + 0xc_5000;
pub const ACTOR_ID: u32 = 0x0012_3456;
const IS_SELF: u32 = 0x7200_0000;
const NOTHING: u32 = 0x7200_0010;
const RTTI_NODE: u32 = 0x7200_0030;
const RTTI_BLEND: u32 = 0x7200_0034;
/// The skinned geometry: its vtable, its skin instance (`+0xbc`), the skin's data (`+0x08`) and
/// bone list (`+0x14`), the data's bone count (`+0x44`).
const GEOMETRY: u32 = BASE + 0xc_6000;
const GEOMETRY_VTABLE: u32 = BASE + 0xc_6100;
const SKIN: u32 = BASE + 0xc_6200;
const SKIN_DATA_AT: u32 = BASE + 0xc_6300;
const SKIN_BONE_LIST: u32 = BASE + 0xc_6400;
const GEOMETRY_SKIN: u32 = 0xbc;
const SKIN_DATA: u32 = 0x08;
const SKIN_BONES: u32 = 0x14;
const SKIN_DATA_BONES: u32 = 0x44;
/// `IsNode` and `IsGeometry` (vtable `+0x0c`, `+0x18`).
const IS_NODE_SLOT: u32 = 0x0c;
const IS_GEOMETRY_SLOT: u32 = 0x18;

/// The world under the actor as the doubles answer it: the collision floor's height, the
/// navmesh's (game units).
#[derive(Clone, Copy, Debug)]
pub struct Ground {
    pub collision: Option<f32>,
    pub navmesh: f32,
}

/// The engine with the tree laid out: each node's address (the list's order) and the 3D's root
/// (the first node).
pub struct Fixture {
    pub e: Engine,
    pub nodes: Vec<Node>,
    pub at: Vec<u32>,
    /// Other actors' trees laid out beside it ([`Fixture::add_actor`]): each actor, its form id,
    /// its nodes' addresses and its nodes.
    pub others: Vec<(u32, u32, Vec<u32>, Vec<Node>)>,
    /// Each node's world as the last [`Fixture::animate`] made it (the list's order), before New
    /// Physics' snapshot hook (which then puts its body's worlds in).
    pub anim: Vec<(M3, V3)>,
}

thread_local! {
    /// The collision floor the tests' casts meet (`crate::active_ragdoll::floor_under`).
    pub static FLOOR: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
    /// Its slope (a point's x and y, the rise along x and along y: units a unit), none flat.
    pub static SLOPE: std::cell::Cell<Option<[f32; 4]>> = const { std::cell::Cell::new(None) };
    /// Bumps on it, as a trench's dirt (a point's x and y, the height and the length of a bump:
    /// game units), none smooth.
    pub static BUMPS: std::cell::Cell<Option<[f32; 4]>> = const { std::cell::Cell::new(None) };
    /// The ground as cast in the game around a spot (its x and y, then 5 by 5 heights against it,
    /// row by row along y, `crate::physics_replay::GRID_SPACING` apart), its edge's heights
    /// beyond; none: none.
    pub static GRID: std::cell::RefCell<Option<([f32; 2], Vec<f32>)>> =
        const { std::cell::RefCell::new(None) };
    /// The world the tests' ray casts meet (`crate::active_ragdoll`'s `cast_world`): the floor
    /// and these boxes (each its least and most corner, game units: a wall, a crate, a slab);
    /// none: the casts find nothing, as before.
    pub static WORLD: std::cell::RefCell<Option<Vec<(V3, V3)>>> =
        const { std::cell::RefCell::new(None) };
    /// Whether the fixture's actor holds a weapon drawn (`Actor::IsWeaponOut`).
    pub static ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The ground under `at` (the floor, tilted by [`SLOPE`]), boxes aside.
fn ground_at(at: V3) -> Option<f32> {
    let f = FLOOR.with(std::cell::Cell::get)?;
    let bumps = BUMPS
        .with(std::cell::Cell::get)
        .map_or(0.0, |[x0, y0, h, l]| {
            let k = std::f32::consts::TAU / l.max(1.0);
            h * ((at[0] - x0) * k).sin() * ((at[1] - y0) * k * 0.77).cos()
        });
    let grid = GRID.with(|g| {
        g.borrow().as_ref().map_or(0.0, |(c, h)| {
            let k = crate::physics_replay::GRID_SPACING;
            let u = ((at[0] - c[0]) / k + 2.0).clamp(0.0, 4.0);
            let v = ((at[1] - c[1]) / k + 2.0).clamp(0.0, 4.0);
            let (i, j) = ((u.floor() as usize).min(3), (v.floor() as usize).min(3));
            let (fu, fv) = (u - i as f32, v - j as f32);
            let z = |i: usize, j: usize| h.get(j * 5 + i).copied().unwrap_or(0.0);
            (z(i, j) * (1.0 - fu) + z(i + 1, j) * fu) * (1.0 - fv)
                + (z(i, j + 1) * (1.0 - fu) + z(i + 1, j + 1) * fu) * fv
        })
    });
    Some(
        bumps
            + grid
            + match SLOPE.with(std::cell::Cell::get) {
                Some([x0, y0, gx, gy]) => f + gx * (at[0] - x0) + gy * (at[1] - y0),
                None => f,
            },
    )
}

/// The tests' collision floor under `at` ([`FLOOR`], tilted by [`SLOPE`]): with a [`WORLD`], the
/// first surface a cast down from 50 units over `at` meets (a box's top included).
pub fn floor_at(at: V3) -> Option<f32> {
    let ground = ground_at(at)?;
    let from = at[2] + 50.0;
    let top = WORLD.with(|w| {
        w.borrow().as_ref().and_then(|boxes| {
            boxes
                .iter()
                .filter(|(lo, hi)| {
                    at[0] >= lo[0] && at[0] <= hi[0] && at[1] >= lo[1] && at[1] <= hi[1]
                })
                .map(|(_, hi)| hi[2])
                .filter(|&z| z <= from)
                .reduce(f32::max)
        })
    });
    Some(top.map_or(ground, |t| t.max(ground)))
}

/// A ray from `from` along `dir` (unit) within `range` against the [`WORLD`]'s boxes and its
/// floor: the nearest hit point; none without a world (or nothing hit). A box the ray starts in
/// is passed (as Havok's casts pass a shape they start inside).
pub fn cast_at(from: V3, dir: V3, range: f32) -> Option<Option<V3>> {
    WORLD.with(|w| {
        let w = w.borrow();
        let boxes = w.as_ref()?;
        let mut best: Option<f32> = None;
        for (lo, hi) in boxes {
            let inside = (0..3).all(|i| from[i] > lo[i] && from[i] < hi[i]);
            if inside {
                continue;
            }
            let (mut t0, mut t1) = (0.0f32, range);
            let mut hit = true;
            for i in 0..3 {
                if dir[i].abs() < 1e-9 {
                    if from[i] < lo[i] || from[i] > hi[i] {
                        hit = false;
                        break;
                    }
                } else {
                    let (a, b) = ((lo[i] - from[i]) / dir[i], (hi[i] - from[i]) / dir[i]);
                    t0 = t0.max(a.min(b));
                    t1 = t1.min(a.max(b));
                }
            }
            if hit && t0 <= t1 {
                best = Some(best.map_or(t0, |b| b.min(t0)));
            }
        }
        // The floor: marched in short steps (it may be sloped).
        let mut t = 0.0f32;
        while t <= range {
            let p = [
                from[0] + dir[0] * t,
                from[1] + dir[1] * t,
                from[2] + dir[2] * t,
            ];
            if ground_at(p).is_some_and(|g| p[2] < g) {
                best = Some(best.map_or(t, |b| b.min(t)));
                break;
            }
            t += 1.0;
        }
        Some(best.map(|t| {
            [
                from[0] + dir[0] * t,
                from[1] + dir[1] * t,
                from[2] + dir[2] * t,
            ]
        }))
    })
}

impl Fixture {
    /// The tree laid out, its worlds worked out, the actor standing at its reference (0, 0, 0)
    /// on a navmesh at `ground`.
    pub fn new(nodes: Vec<Node>, ground: Ground) -> Fixture {
        let mut e = Engine::new();
        e.mem.map(BASE, SIZE);
        let page = HK_TO_GAME & !0xfff;
        e.mem.map(page, 0x1000);
        e.mem.set_f32(HK_TO_GAME, HK);
        // And its inverse, the game-to-Havok scale (`011c582c`, what `NI2HK` multiplies by).
        if !e.mem.is_mapped(GAME_TO_HK & !0xfff) {
            e.mem.map(GAME_TO_HK & !0xfff, 0x1000);
        }
        e.mem.set_f32(GAME_TO_HK, 1.0 / HK);
        // The process manager's lists (`jip::commands::jip_fn_effect::high_actors`): empty.
        e.mem.map(0x011e_0000, 0x2000);
        // Whether an AI thread's task runs now (`008c7aa0`): none, unless a test says so.
        e.register_double(0x008c_7aa0, |_, _| 0u32.into_ret());
        FLOOR.with(|f| f.set(ground.collision));
        SLOPE.with(|f| f.set(None));
        BUMPS.with(|f| f.set(None));
        GRID.with(|g| *g.borrow_mut() = None);
        WORLD.with(|w| *w.borrow_mut() = None);
        ARMED.with(|a| a.set(false));
        let at: Vec<u32> = (0..nodes.len() as u32)
            .map(|i| BASE + i * NODE_SIZE)
            .collect();
        let mut name_at = NAMES;
        for (i, n) in nodes.iter().enumerate() {
            let node = at[i];
            e.mem.set_u32(node, NODE_VTABLE);
            e.mem.set_u32(node + PARENT, n.parent.map_or(0, |p| at[p]));
            e.mem.set_cstr(name_at, n.name.as_bytes());
            e.mem.set_u32(node + NODE_NAME, name_at);
            name_at += (n.name.len() as u32 + 16) & !15;
            write9(&mut e, node + LOCAL_ROTATION, &n.r);
            write3(&mut e, node + LOCAL_TRANSLATION, n.t);
            e.mem.set_f32(node + LOCAL_SCALE, n.s);
        }
        // The node walk: a node is its own children array; its children are the nodes whose
        // parent it is.
        let mut kids: Vec<Vec<u32>> = (0..nodes.len())
            .map(|i| {
                (0..nodes.len())
                    .filter(|&c| nodes[c].parent == Some(i))
                    .map(|c| at[c])
                    .collect()
            })
            .collect();
        // The body's skinned geometry under the root, its skin listing every `Bip01` node (what
        // the drawing skins with: `crate::active_ragdoll`'s read at the world pass).
        let skinned: Vec<u32> = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.name.starts_with("Bip01 "))
            .map(|(i, _)| at[i])
            .collect();
        e.mem.set_u32(GEOMETRY, GEOMETRY_VTABLE);
        e.mem.set_u32(GEOMETRY + GEOMETRY_SKIN, SKIN);
        e.mem.set_u32(SKIN + SKIN_DATA, SKIN_DATA_AT);
        e.mem.set_u32(SKIN + SKIN_BONES, SKIN_BONE_LIST);
        e.mem
            .set_u32(SKIN_DATA_AT + SKIN_DATA_BONES, skinned.len() as u32);
        for (k, b) in skinned.iter().enumerate() {
            e.mem.set_u32(SKIN_BONE_LIST + 4 * k as u32, *b);
        }
        e.mem.set_u32(GEOMETRY_VTABLE + IS_NODE_SLOT, NOTHING);
        e.mem.set_u32(GEOMETRY_VTABLE + IS_GEOMETRY_SLOT, IS_SELF);
        kids[0].push(GEOMETRY);
        let index = at.clone();
        let kids2 = kids.clone();
        let find = move |node: u32| index.iter().position(|&a| a == node);
        let find2 = find.clone();
        e.register_double(IS_SELF, |_, a| a[0].into_ret());
        e.register_double(NOTHING, |_, _| 0u32.into_ret());
        e.mem.set_u32(NODE_VTABLE + IS_NODE_SLOT, IS_SELF);
        e.mem.set_u32(NODE_VTABLE + IS_GEOMETRY_SLOT, NOTHING);
        e.register_double(CHILD_COUNT, move |_, a| {
            find(a[0]).map_or(0, |i| kids[i].len() as u32).into_ret()
        });
        e.register_double(CHILD_AT, move |_, a| {
            find2(a[0])
                .and_then(|i| kids2[i].get(a[1] as usize).copied())
                .unwrap_or(0)
                .into_ret()
        });
        let order = at.clone();
        e.register_double(UPDATE_DATA, |_, a| a[0].into_ret());
        e.register_double(NODE_UPDATE, move |e, _| {
            update_worlds(e, &order);
            0u32.into_ret()
        });
        let root = at[0];
        e.register_double(ACTOR_3D, move |_, a| {
            if a[0] == ACTOR { root } else { 0 }.into_ret()
        });
        e.register_double(LOOKUP_FORM, |_, a| {
            if a[0] == ACTOR_ID { ACTOR } else { 0 }.into_ret()
        });
        e.register_double(GET_SCALE, |_, _| 1.0f32.into_ret());
        e.register_double(IS_WEAPON_DRAWN, |_, _| {
            u32::from(ARMED.with(std::cell::Cell::get)).into_ret()
        });
        // The ragdoll's bodies: a node with one answers a collision object (its body block), the
        // collision object its rigid body, the rigid body its `hkpRigidBody` (the same block);
        // velocities kept in the block (Havok units).
        let with_body: Vec<(u32, u32)> = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.body)
            .enumerate()
            .map(|(k, (i, _))| (at[i], BODIES + k as u32 * BODY_SIZE))
            .collect();
        let wb = with_body.clone();
        e.register_double(NODE_COLLISION_OBJECT, move |_, a| {
            wb.iter()
                .find(|(n, _)| *n == a[0])
                .map_or(0, |(_, b)| *b)
                .into_ret()
        });
        e.register_double(COLLISION_RIGID_BODY, |_, a| a[0].into_ret());
        e.register_double(HAVOK_BODY, |_, a| a[0].into_ret());
        e.register_double(LINEAR_VELOCITY, |_, a| (a[0] + 0x10).into_ret());
        e.register_double(ANGULAR_VELOCITY, |_, a| (a[0] + 0x20).into_ret());
        // A body moved (`hkpRigidBody::setPositionAndRotation`'s queueing wrapper, `00ca8960`):
        // its position (`+0x120`) and rotation quaternion (`+0x160`), Havok units, as
        // `crate::knockdown` reads them; the fixture's bodies have no Havok entity of their own.
        e.register_double(BODY_SET_POSITION_AND_ROTATION, |e, a| {
            let p = read3(e, a[1]);
            write3(e, a[0] + BODY_TRANSLATION, p);
            for i in 0..4 {
                let x = e.mem.f32(a[2] + 4 * i);
                e.mem.set_f32(a[0] + BODY_ROTATION + 4 * i, x);
            }
            0u32.into_ret()
        });
        e.register_double(SET_LINEAR_VELOCITY, |e, a| {
            let v = read3(e, a[1]);
            write3(e, a[0] + 0x10, v);
            0u32.into_ret()
        });
        e.register_double(SET_ANGULAR_VELOCITY, |e, a| {
            let v = read3(e, a[1]);
            write3(e, a[0] + 0x20, v);
            0u32.into_ret()
        });
        for (_, b) in &with_body {
            e.mem.set_u8(b + MOTION_TYPE, 1);
        }
        // The actor, its process (middle-high, standing), its character controller.
        e.mem.set_u32(ACTOR + FORM_ID, ACTOR_ID);
        e.mem.set_u32(ACTOR + PROCESS, PROCESS_AT);
        e.mem.set_u32(PROCESS_AT, PROCESS_VTABLE);
        e.mem.set_u32(PROCESS_VTABLE + 0x3e4, NOTHING);
        e.register_double(SET_LOCATION, |e, a| {
            let p = read3(e, a[1]);
            write3(e, a[0] + REFR_POSITION, p);
            0u32.into_ret()
        });
        e.register_double(CHAR_CONTROLLER, |_, _| CONTROLLER.into_ret());
        e.register_double(CONTROLLER_SET_POSITION, |_, _| 0u32.into_ret());
        e.register_double(CONTROLLER_SET_VELOCITY, |e, a| {
            let v = read3(e, a[1]);
            // The game's `SetLinearVelocity` takes game units and keeps Havok units
            // (`00926980` through `004a3e00`); `GethkLinearVelocity` hands those back.
            write3(e, CONTROLLER + 0x700, v.map(|x| x / HK));
            0u32.into_ret()
        });
        e.register_double(CONTROLLER_VELOCITY, |_, _| (CONTROLLER + 0x700).into_ret());
        // The navmesh: under any point at its height.
        let nav = ground.navmesh;
        e.register_double(PATHING_LOCATION_AT, move |e, a| {
            let p = read3(e, a[1]);
            write3(e, a[0] + PATHING_POSITION, [p[0], p[1], nav]);
            0u32.into_ret()
        });
        e.register_double(RESOLVE_TO_NAVMESH, |_, _| 1u32.into_ret());
        e.register_double(IS_VALID_LOCATION, |_, _| 1u32.into_ret());
        let _ = PATHING;
        update_worlds(&mut e, &at);
        // Each body where its node is (the keyframed bodies of a living actor follow the
        // animation): its position (Havok units) and rotation.
        for &(node, body) in &with_body {
            let p = read3(&e, node + WORLD_TRANSLATION).map(|x| x / HK);
            write3(&mut e, body + BODY_TRANSLATION, p);
            let q = quaternion(&read9(&e, node + WORLD_ROTATION));
            for (i, x) in q.into_iter().enumerate() {
                e.mem.set_f32(body + BODY_ROTATION + 4 * i as u32, x);
            }
        }
        Fixture {
            e,
            nodes,
            at,
            others: Vec::new(),
            anim: Vec::new(),
        }
    }

    /// Another actor (`actor`, already laid out by the caller: its vtable, process and the rest)
    /// given a tree of its own: `nodes` laid out at `base` (mapped here, `0x2_0000` bytes: the
    /// nodes, their names, their child lists), each an `NiNode` as the game's update reads it (no
    /// collision object), its 3D's root answered for it, its form id found, the node walk and the
    /// update covering both trees. Call before [`Fixture::game_update`].
    pub fn add_actor(&mut self, actor: u32, id: u32, nodes: Vec<Node>, base: u32) -> Vec<u32> {
        let e = &mut self.e;
        e.mem.map(base, 0x2_0000);
        let at: Vec<u32> = (0..nodes.len() as u32)
            .map(|i| base + i * NODE_SIZE)
            .collect();
        let mut name_at = base + 0x1_0000;
        let mut list = base + 0x1_8000;
        for (i, n) in nodes.iter().enumerate() {
            let node = at[i];
            e.mem.set_u32(node, NODE_VTABLE);
            e.mem.set_u32(node + PARENT, n.parent.map_or(0, |p| at[p]));
            e.mem.set_cstr(name_at, n.name.as_bytes());
            e.mem.set_u32(node + NODE_NAME, name_at);
            name_at += (n.name.len() as u32 + 16) & !15;
            write9(e, node + LOCAL_ROTATION, &n.r);
            write3(e, node + LOCAL_TRANSLATION, n.t);
            e.mem.set_f32(node + LOCAL_SCALE, n.s);
            let kids: Vec<u32> = (0..nodes.len())
                .filter(|&c| nodes[c].parent == Some(i))
                .map(|c| at[c])
                .collect();
            for (k, c) in kids.iter().enumerate() {
                e.mem.set_u32(list + 4 * k as u32, *c);
            }
            e.mem.set_u32(node + 0xa0, list);
            e.mem.set_u16(node + 0xa6, kids.len() as u16);
            e.mem.set_u32(node + 0x1c, 0);
            list += 4 * (kids.len() as u32 + 1);
        }
        update_worlds(e, &at);
        self.others.push((actor, id, at.clone(), nodes));
        // The doubles that answer per tree, now for every tree.
        let mut trees: Vec<Tree> = vec![(
            ACTOR,
            ACTOR_ID,
            self.at.clone(),
            self.nodes.iter().map(|n| n.parent).collect(),
        )];
        for (a, i, at, nodes) in &self.others {
            trees.push((*a, *i, at.clone(), nodes.iter().map(|n| n.parent).collect()));
        }
        let mut kids: std::collections::HashMap<u32, Vec<u32>> = Default::default();
        for (_, _, at, parents) in &trees {
            for (i, &node) in at.iter().enumerate() {
                kids.insert(
                    node,
                    (0..at.len())
                        .filter(|&c| parents[c] == Some(i))
                        .map(|c| at[c])
                        .collect(),
                );
            }
        }
        kids.entry(self.at[0]).or_default().push(GEOMETRY);
        let kids2 = kids.clone();
        let e = &mut self.e;
        e.register_double(CHILD_COUNT, move |_, a| {
            kids.get(&a[0]).map_or(0, |k| k.len() as u32).into_ret()
        });
        e.register_double(CHILD_AT, move |_, a| {
            kids2
                .get(&a[0])
                .and_then(|k| k.get(a[1] as usize).copied())
                .unwrap_or(0)
                .into_ret()
        });
        let roots: Vec<(u32, u32, u32)> = trees.iter().map(|t| (t.0, t.1, t.2[0])).collect();
        let roots2 = roots.clone();
        e.register_double(ACTOR_3D, move |_, a| {
            roots
                .iter()
                .find(|r| r.0 == a[0])
                .map_or(0, |r| r.2)
                .into_ret()
        });
        e.register_double(LOOKUP_FORM, move |_, a| {
            roots2
                .iter()
                .find(|r| r.1 == a[0])
                .map_or(0, |r| r.0)
                .into_ret()
        });
        let orders: Vec<Vec<u32>> = trees.iter().map(|t| t.2.clone()).collect();
        e.register_double(NODE_UPDATE, move |e, a| {
            for order in &orders {
                if order.contains(&a[0]) {
                    update_worlds(e, order);
                }
            }
            0u32.into_ret()
        });
        at
    }

    /// The game's own update in place of the double (`NiAVObject::Update`, `00a59c60`, and what
    /// it runs, translated): each node an `NiNode` as the update reads it (its children at
    /// `+0xa0`/`+0xa6`, no flags, no bound), and each node with a ragdoll body given a
    /// `bhkBlendCollisionObject` at `+0x1c` as FNV's skeleton has: `NiAVObject::UpdateWorldData`
    /// (`00a68c60`) hands such a node's world to it (its `+0x90`, `00c817d0`), which does nothing
    /// while the thread block's word at `+0x2c0` is 3 (the no-controller update's "nodes only").
    pub fn game_update(&mut self) {
        const CHILD_LISTS: u32 = BASE + 0xd_0000;
        const BLENDS: u32 = BASE + 0xe_0000;
        const BLEND_VTABLE: u32 = BASE + 0xe_f000;
        let e = &mut self.e;
        for (addr, f) in [
            engine::entry!(
                0x00a5_9c60,
                engine::units::nimain::run_00a59c60::fn_00a59c60(Ptr, u32)
            ),
            engine::entry!(
                0x00a5_dd70,
                engine::units::nimain::ninode::ni_node_update_downward_pass(Ptr, u32, u32)
            ),
            engine::entry!(
                0x00a5_db30,
                engine::units::nimain::ninode::ni_node_update_upward_pass(Ptr)
            ),
            engine::entry!(
                0x00a5_e2d0,
                engine::units::nimain::ninode::ni_node_update_world_bound(Ptr)
            ),
            engine::entry!(
                0x00a6_8c60,
                engine::units::nimain::niavobject_xenon::ni_av_object_update_world_data(
                    Ptr<engine::types::NiAVObject>,
                    u32
                )
            ),
            engine::entry!(
                0x00a6_8bf0,
                engine::units::nimain::run_00a68bf0::fn_00a68bf0(
                    Ptr<engine::types::NiAVObject>
                )
            ),
            engine::entry!(
                0x0062_c250,
                engine::units::fallout_shared_havok::teshavokutilities::fn_0062c250(
                    Ptr, Ptr, Ptr
                ) -> Ptr
            ),
            engine::entry!(
                0x00c8_17d0,
                engine::units::bshavok::bhkblendcollisionobject::bhk_blend_collision_object_update_world_data(
                    Ptr, u32
                )
            ),
        ] {
            e.register(addr, f);
        }
        // An update data of no time, no controllers (`NiUpdateData`: its byte `+4` clear).
        e.register_double(UPDATE_DATA, |e, a| {
            for i in 0..4 {
                e.mem.set_u32(a[0] + 4 * i, 0);
            }
            a[0].into_ret()
        });
        e.mem.set_u32(NODE_VTABLE + 0xa4, 0x00a5_dd70);
        e.mem.set_u32(NODE_VTABLE + 0xb8, 0x00a6_8c60);
        e.mem.set_u32(NODE_VTABLE + 0xbc, 0x00a5_e2d0);
        e.mem.set_u32(NODE_VTABLE + 0xfc, 0x00a5_db30);
        e.mem.set_u32(GEOMETRY_VTABLE + 0xa4, NOTHING);
        e.mem.set_u32(BLEND_VTABLE + 0x90, 0x00c8_17d0);
        // Their types (`GetRTTI`, vtable `+8`: an `NiRTTI`, its name at `+0`, its parent's at
        // `+4`): an `NiNode`, and a blend collision object that is not Havok's shape holder (the
        // fixture has no Havok shapes: a ray the game casts at the bodies meets none).
        const RTTI: u32 = BASE + 0xe_f800;
        e.mem.set_cstr(RTTI + 0x80, b"NiNode");
        e.mem.set_cstr(RTTI + 0x90, b"bhkBlendCollisionObject");
        e.mem.set_u32(RTTI, RTTI + 0x80);
        e.mem.set_u32(RTTI + 0x10, RTTI + 0x90);
        e.register_double(RTTI_NODE, |_, _| RTTI.into_ret());
        e.register_double(RTTI_BLEND, |_, _| (RTTI + 0x10).into_ret());
        e.mem.set_u32(NODE_VTABLE + 8, RTTI_NODE);
        e.mem.set_u32(BLEND_VTABLE + 8, RTTI_BLEND);
        let mut list = CHILD_LISTS;
        let mut blend = BLENDS;
        for (i, n) in self.nodes.iter().enumerate() {
            let node = self.at[i];
            let mut kids: Vec<u32> = (0..self.nodes.len())
                .filter(|&c| self.nodes[c].parent == Some(i))
                .map(|c| self.at[c])
                .collect();
            if i == 0 {
                kids.push(GEOMETRY);
            }
            for (k, c) in kids.iter().enumerate() {
                e.mem.set_u32(list + 4 * k as u32, *c);
            }
            e.mem.set_u32(node + 0xa0, list);
            e.mem.set_u16(node + 0xa6, kids.len() as u16);
            e.mem.set_u32(node + 0x30, 0);
            e.mem.set_u32(node + 0x20, 0);
            list += 4 * (kids.len() as u32 + 1);
            if n.body {
                e.mem.set_u32(blend, BLEND_VTABLE);
                e.mem.set_u32(blend + 8, node);
                e.mem.set_u32(node + 0x1c, blend);
                blend += 0x40;
            } else {
                e.mem.set_u32(node + 0x1c, 0);
            }
        }
    }

    /// The node named `name` (its address), when there is one.
    pub fn node(&self, name: &str) -> Option<u32> {
        self.nodes
            .iter()
            .position(|n| n.name.eq_ignore_ascii_case(name))
            .map(|i| self.at[i])
    }

    /// The world rotation and translation of the node named `name`.
    pub fn world(&self, name: &str) -> Option<(M3, V3)> {
        let n = self.node(name)?;
        Some((
            read9(&self.e, n + WORLD_ROTATION),
            read3(&self.e, n + WORLD_TRANSLATION),
        ))
    }

    /// The actor's knock state.
    pub fn set_knock_state(&mut self, state: u8) {
        self.e.mem.set_u8(PROCESS_AT + KNOCK_STATE, state);
    }

    /// The worlds worked out again from the locals (the game's update).
    pub fn update(&mut self) {
        let at = self.at.clone();
        update_worlds(&mut self.e, &at);
    }

    /// Each local as the animation has it (the tree's own; the root's, where the actor is, as
    /// it is), the worlds worked out: the game's animation update (New Physics' snapshot taken
    /// after it, `crate::new_physics::animation_updated`).
    pub fn animate(&mut self) {
        for i in 1..self.nodes.len() {
            let (r, t) = (self.nodes[i].r, self.nodes[i].t);
            write9(&mut self.e, self.at[i] + LOCAL_ROTATION, &r);
            write3(&mut self.e, self.at[i] + LOCAL_TRANSLATION, t);
        }
        self.update();
        self.anim = self.worlds();
        // The game's animation update ends here: New Physics' snapshot of it.
        crate::new_physics::animation_updated(&mut self.e, ACTOR);
    }

    /// The bodies' velocities (game units a second) by node name, as the doubles keep them.
    pub fn body_velocities(&self) -> Vec<(String, V3)> {
        let mut k = 0u32;
        let mut out = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if n.body {
                let v = read3(&self.e, BODIES + k * BODY_SIZE + 0x10);
                out.push((self.nodes[i].name.clone(), scale(v, HK)));
                k += 1;
            }
        }
        out
    }

    /// Every node's world (rotation, translation), in the list's order.
    pub fn worlds(&self) -> Vec<(M3, V3)> {
        self.at
            .iter()
            .map(|&n| {
                (
                    read9(&self.e, n + WORLD_ROTATION),
                    read3(&self.e, n + WORLD_TRANSLATION),
                )
            })
            .collect()
    }

    /// Every node's world set to `worlds` (the list's order; the root's kept), each local worked
    /// out from its parent's new world (as the ragdoll's sync leaves a node it moves).
    pub fn set_worlds(&mut self, worlds: &[(M3, V3)]) {
        let at = self.at.clone();
        for (&n, &(r, t)) in at.iter().zip(worlds).skip(1) {
            let parent = self.e.mem.u32(n + PARENT);
            let (pr, pt, ps) = (
                read9(&self.e, parent + WORLD_ROTATION),
                read3(&self.e, parent + WORLD_TRANSLATION),
                self.e.mem.f32(parent + WORLD_SCALE),
            );
            let lr = mul(&transpose(&pr), &r);
            let lt = scale(apply_m(&transpose(&pr), sub(t, pt)), 1.0 / ps.max(1e-3));
            write9(&mut self.e, n + LOCAL_ROTATION, &lr);
            write3(&mut self.e, n + LOCAL_TRANSLATION, lt);
            write9(&mut self.e, n + WORLD_ROTATION, &r);
            write3(&mut self.e, n + WORLD_TRANSLATION, t);
        }
    }

    /// Every node's world set to `worlds`, the locals left as they are (as the ragdoll's sync
    /// draws a node the animation has the local of).
    pub fn set_worlds_only(&mut self, worlds: &[(M3, V3)]) {
        let at = self.at.clone();
        for (&n, (r, t)) in at.iter().zip(worlds).skip(1) {
            write9(&mut self.e, n + WORLD_ROTATION, r);
            write3(&mut self.e, n + WORLD_TRANSLATION, *t);
        }
    }

    /// The indices (in the list) of the nodes a ragdoll body hangs from, in the bodies' order.
    pub fn body_nodes(&self) -> Vec<usize> {
        (0..self.nodes.len())
            .filter(|&i| self.nodes[i].body)
            .collect()
    }

    /// The `k`th body's linear velocity (game units a second), and set.
    pub fn body_velocity(&self, k: usize) -> V3 {
        scale(read3(&self.e, BODIES + k as u32 * BODY_SIZE + 0x10), HK)
    }
    /// The `k`th body's spin (rad/s), as the doubles keep it.
    pub fn body_spin(&self, k: usize) -> V3 {
        read3(&self.e, BODIES + k as u32 * BODY_SIZE + 0x20)
    }
    pub fn set_body_spin(&mut self, k: usize, w: V3) {
        write3(&mut self.e, BODIES + k as u32 * BODY_SIZE + 0x20, w);
    }
    pub fn set_body_velocity(&mut self, k: usize, v: V3) {
        write3(
            &mut self.e,
            BODIES + k as u32 * BODY_SIZE + 0x10,
            scale(v, 1.0 / HK),
        );
    }

    /// The animation's world of the node at `node` (as the last [`Fixture::animate`] made it).
    pub fn anim_world_of(&self, node: u32) -> (M3, V3) {
        let i = self
            .at
            .iter()
            .position(|&n| n == node)
            .expect("a node of the tree");
        self.anim[i]
    }

    /// The world (rotation, translation) of the node at `node`.
    pub fn world_of(&self, node: u32) -> (M3, V3) {
        (
            read9(&self.e, node + WORLD_ROTATION),
            read3(&self.e, node + WORLD_TRANSLATION),
        )
    }

    /// The actor a character (its vtable's `IsCharacter`, `+0x100`, answering yes, and
    /// `GetCombatTarget`, `+0x42c`, none): New Physics takes only characters.
    pub fn make_character(&mut self) {
        const ACTOR_VTABLE: u32 = BASE + 0xc_7000;
        const YES: u32 = 0x7200_0020;
        const NONE: u32 = 0x7200_0024;
        self.e.register_double(YES, |_, _| 1u32.into_ret());
        self.e.register_double(NONE, |_, _| 0u32.into_ret());
        self.e.mem.set_u32(ACTOR, ACTOR_VTABLE);
        self.e.mem.set_u32(ACTOR_VTABLE + 0x100, YES);
        // `GetCombatTarget` (`+0x42c`): none.
        self.e.mem.set_u32(ACTOR_VTABLE + 0x42c, NONE);
        // Its position (the knockdowns' `position`): the reference's.
        const POSITION: u32 = 0x7200_002c;
        self.e
            .register_double(POSITION, |_, _| (ACTOR + REFR_POSITION).into_ret());
        self.e.mem.set_u32(ACTOR_VTABLE + 0x1f4, POSITION);
        // The player's pointer (`0x011dea3c`): none.
        self.e.mem.map(0x011d_e000, 0x1000);
    }

    /// The actor's reference position.
    pub fn reference(&self) -> V3 {
        read3(&self.e, ACTOR + REFR_POSITION)
    }
}

/// The unit quaternion (x, y, z, w) of a rotation matrix (row-major, as `NiMatrix3`).
pub fn quaternion(m: &M3) -> [f32; 4] {
    let trace = m[0] + m[4] + m[8];
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (m[7] - m[5]) / s,
            (m[2] - m[6]) / s,
            (m[3] - m[1]) / s,
            0.25 * s,
        ]
    } else if m[0] > m[4] && m[0] > m[8] {
        let s = (1.0 + m[0] - m[4] - m[8]).sqrt() * 2.0;
        [
            0.25 * s,
            (m[1] + m[3]) / s,
            (m[2] + m[6]) / s,
            (m[7] - m[5]) / s,
        ]
    } else if m[4] > m[8] {
        let s = (1.0 + m[4] - m[0] - m[8]).sqrt() * 2.0;
        [
            (m[1] + m[3]) / s,
            0.25 * s,
            (m[5] + m[7]) / s,
            (m[2] - m[6]) / s,
        ]
    } else {
        let s = (1.0 + m[8] - m[0] - m[4]).sqrt() * 2.0;
        [
            (m[2] + m[6]) / s,
            (m[5] + m[7]) / s,
            0.25 * s,
            (m[3] - m[1]) / s,
        ]
    }
}

/// The worlds of the nodes at `order` (parents first) from their locals.
pub fn update_worlds(e: &mut Engine, order: &[u32]) {
    for &node in order {
        let parent = e.mem.u32(node + PARENT);
        let r = read9(e, node + LOCAL_ROTATION);
        let t = read3(e, node + LOCAL_TRANSLATION);
        let s = e.mem.f32(node + LOCAL_SCALE);
        let (wr, wt, ws) = if parent == 0 {
            (r, t, s)
        } else {
            let pr = read9(e, parent + WORLD_ROTATION);
            let pt = read3(e, parent + WORLD_TRANSLATION);
            let ps = e.mem.f32(parent + WORLD_SCALE);
            (mul(&pr, &r), add(apply_m(&pr, scale(t, ps)), pt), ps * s)
        };
        write9(e, node + WORLD_ROTATION, &wr);
        write3(e, node + WORLD_TRANSLATION, wt);
        e.mem.set_f32(node + WORLD_SCALE, ws);
    }
}

pub fn read3(e: &Engine, at: u32) -> V3 {
    [0, 4, 8].map(|o| e.mem.f32(at + o))
}
pub fn read9(e: &Engine, at: u32) -> M3 {
    std::array::from_fn(|i| e.mem.f32(at + 4 * i as u32))
}
pub fn write3(e: &mut Engine, at: u32, v: V3) {
    for (i, x) in v.into_iter().enumerate() {
        e.mem.set_f32(at + 4 * i as u32, x);
    }
}
pub fn write9(e: &mut Engine, at: u32, m: &M3) {
    for (i, x) in m.iter().enumerate() {
        e.mem.set_f32(at + 4 * i as u32, *x);
    }
}

/// The angle (degrees) between two rotations.
pub fn degrees_between(a: &M3, b: &M3) -> f32 {
    length(ragdoll_core::flinch::rotation_vector(&mul(
        a,
        &transpose(b),
    )))
    .to_degrees()
}

/// The lab's mannequin (`ragdoll_core::flinch::BONES` at the lab's joints) laid out as FNV's tree
/// is (`Bip01` > `Bip01 NonAccum` > the pelvis, and the spine beside it), every bone's rest
/// rotation a turn of its own (none the identity), and a ragdoll body on each rig bone but the
/// pelvis (as the game's: a7a2d15 found none on it). For the tests that need no dump.
pub fn mannequin() -> Vec<Node> {
    use ragdoll_core::flinch::BONES;
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
    const UP: [Option<usize>; 20] = [
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
    let rest = |k: f32| {
        let along = ragdoll_core::flinch::rotation([
            0.0,
            -std::f32::consts::FRAC_PI_2 + 0.3 * k.sin(),
            0.0,
        ]);
        let about = ragdoll_core::flinch::rotation([0.7 * k.cos(), 0.0, 0.5 * (2.0 * k).sin()]);
        mul(&about, &along)
    };
    let non_accum_r = rest(0.37);
    let mut out = vec![
        Node {
            name: "Scene Root".into(),
            parent: None,
            r: IDENTITY,
            t: [0.0; 3],
            s: 1.0,
            body: false,
        },
        Node {
            name: "Bip01 NonAccum".into(),
            parent: Some(0),
            r: non_accum_r,
            t: [0.0, 0.0, 66.0],
            s: 1.0,
            body: false,
        },
    ];
    let world: Vec<M3> = (0..20).map(|b| rest(b as f32 * 0.61 + 0.2)).collect();
    for (b, (name, _)) in BONES.iter().enumerate() {
        let (up_r, up_at) = UP[b].map_or((non_accum_r, [0.0, 0.0, 66.0]), |p| (world[p], JOINT[p]));
        out.push(Node {
            name: (*name).into(),
            parent: Some(UP[b].map_or(1, |p| p + 2)),
            r: mul(&transpose(&up_r), &world[b]),
            t: apply_m(&transpose(&up_r), sub(JOINT[b], up_at)),
            s: 1.0,
            body: b != 0,
        });
    }
    out
}

/// FNV's own skeleton (`meshes\characters\_male\skeleton.nif`, the male and female humans'), from
/// its derived data (`crates/ragdoll-core/data/fnv_skeleton.txt`: node names, parents, local transforms, which
/// node each ragdoll body hangs from): every `NiNode` as the file lays them out, the rows as stored
/// read as the transpose of the matrix the game applies (the left thigh at -X, as the left thigh's
/// body is). No body on `Bip01 Pelvis`; the root body on `Bip01 NonAccum`, the spine its sibling.
pub fn fnv_skeleton() -> Vec<Node> {
    let text = include_str!("../../ragdoll-core/data/fnv_skeleton.txt");
    let mut nodes: Vec<Node> = Vec::new();
    for line in text.lines() {
        if line.starts_with("# RIGID BODIES") {
            break;
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('|').collect();
        if f.len() < 10 || f[4] != "NiNode" && f[4] != "BSFadeNode" {
            continue;
        }
        let nums = |s: &str| -> Vec<f32> {
            s.split([',', ' ', ';'])
                .filter(|x| !x.is_empty())
                .map(|x| x.parse::<f32>().expect("a number"))
                .collect()
        };
        let t = nums(f[5]);
        let rows = nums(f[6]);
        let r: M3 = std::array::from_fn(|k| rows[(k % 3) * 3 + k / 3]);
        let parent = (!f[3].is_empty()).then(|| {
            nodes
                .iter()
                .position(|n| n.name == f[3])
                .unwrap_or_else(|| panic!("{}'s parent {} not before it", f[2], f[3]))
        });
        nodes.push(Node {
            name: f[2].to_string(),
            parent,
            r: ragdoll_core::pose::ortho(&r),
            t: [t[0], t[1], t[2]],
            s: f[8].parse().expect("a scale"),
            body: !f[9].trim().is_empty(),
        });
    }
    nodes
}

/// New Physics' joint limits on the fixture (`ragdoll_core::np_skeleton`, the 17 constraints of the
/// same skeleton): every joint measured from its two body nodes' worlds as the engine's update
/// lays the tree out agrees with the skeleton data's own bind pose within 0.1 degrees, inside its
/// limits but for the neck (20 degrees out of its 6.8 cone, as the data has it).
#[test]
fn the_joint_limits_on_the_fnv_skeleton_fixture() {
    // One fixture at a time (as every New Physics test): the animation's update calls New
    // Physics' hook, which finds an actor by its address in the process's list; beside another
    // test's taken actor at the same address, its drawn pose was written into this fixture's
    // nodes (a thigh 33 degrees off its data).
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    use ragdoll_core::np_skeleton::{Measure, Skeleton};
    let sk = Skeleton::fnv();
    let bind = sk.bind_worlds();
    let mut f = Fixture::new(
        fnv_skeleton(),
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    f.animate();
    let as_vec = |m: Measure| match m {
        Measure::Ragdoll { cone, plane, twist } => [cone, plane, twist],
        Measure::Hinge { angle } => [angle, 0.0, 0.0],
    };
    for j in &sk.joints {
        let (a, b) = (f.world(&j.child).unwrap().0, f.world(&j.parent).unwrap().0);
        let here = j.measure(&a, &b);
        let there = j.measure(
            &bind[sk.node(&j.child).unwrap()].r,
            &bind[sk.node(&j.parent).unwrap()].r,
        );
        for (x, y) in as_vec(here).iter().zip(as_vec(there)) {
            assert!(
                (x - y).abs().to_degrees() < 0.1,
                "{}: {here:?} on the fixture, {there:?} in the data",
                j.child
            );
        }
        let out = j.outside(here).to_degrees();
        if j.child == "Bip01 Neck1" {
            assert!((out - 16.7).abs() < 0.6, "the neck {out:.1} out");
        } else {
            assert!(
                out < 0.05,
                "{} {out:.2} degrees outside its limits",
                j.child
            );
        }
    }
}

/// The fixture's tree is FNV's: the left thigh's joint at -X over the actor's left, the ragdoll's
/// bodies where the file has them (its Havok positions times 7 near their nodes), no body on the
/// pelvis.
#[test]
fn the_fnv_skeleton_is_laid_out_as_the_game_has_it() {
    // One fixture at a time (as every New Physics test): the animation's update calls New
    // Physics' hook, which finds an actor by its address in the process's list; beside another
    // test's taken actor at the same address, its drawn pose was written into this fixture's
    // nodes (a thigh 33 degrees off its data).
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let nodes = fnv_skeleton();
    assert_eq!(nodes.len(), 66, "the 65 NiNodes and the scene root");
    assert_eq!(nodes.iter().filter(|n| n.body).count(), 18);
    let f = Fixture::new(
        nodes,
        Ground {
            collision: Some(0.0),
            navmesh: -8.0,
        },
    );
    let at = |n: &str| f.world(n).unwrap().1;
    let l = at("Bip01 L Thigh");
    assert!(
        (l[0] + 6.98).abs() < 0.05 && (l[2] - 67.77).abs() < 0.1,
        "{l:?}"
    );
    assert!((at("Bip01 R Thigh")[0] - 6.98).abs() < 0.05);
    let spine = at("Bip01 Spine");
    assert!(
        (spine[2] - 73.96).abs() < 0.1 && spine[1] > 1.0,
        "{spine:?}"
    );
    assert!(at("Bip01 Head")[2] > 100.0 && at("Bip01 L Foot")[2] < 12.0);
    assert!(!f.nodes.iter().any(|n| n.name == "Bip01 Pelvis" && n.body));
}
