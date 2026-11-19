//! Patrick's 14:49 crash on c08805c1 (New Physics on): the first pellet on a Gecko while New
//! Physics' actor was stepping, then `HitMe` < `008d4870` < `00406d70` read pi/2 from
//! `[011d61c8 + 4 * index]` and faulted on its `+0x44`.

use crate::new_physics::{claim, owns};
use crate::rig_fixture::{self, Fixture, Ground, ACTOR};
use crate::room_events;
use engine::prelude::*;
use engine::units::fallout_ai::hitdata::HitData;
use ragdoll_core::flinch::{length, sub};

/// The creature's memory (its actor, vtable, process, base form, body parts, the hit): a block of
/// its own beside the fixture's.
const AREA: u32 = 0x7500_0000;
const AREA_SIZE: u32 = 0x4_0000;
pub(crate) const GECKO: u32 = AREA;
const GECKO_ID: u32 = 0xff00_1323;
const GECKO_VTABLE: u32 = AREA + 0x1000;
const GECKO_PROCESS: u32 = AREA + 0x3000;
const PROCESS_VTABLE: u32 = AREA + 0x4000;
const OWNER_VTABLE: u32 = AREA + 0x6000;
const BASE_FORM: u32 = AREA + 0x7000;
const BASE_VTABLE: u32 = AREA + 0x8000;
const BODY_PART_DATA: u32 = AREA + 0xa000;
const BODY_PARTS: u32 = AREA + 0xb000;
const HIT: u32 = AREA + 0xd000;
const CELL: u32 = AREA + 0xe000;
/// The player and the one who fires (an NPC turned hostile), each an actor with the same empty
/// slots and a process.
const PLAYER: u32 = AREA + 0xf000;
const SHOOTER: u32 = AREA + 0xf400;
/// The pellets' weapon (a shotgun's form: its fields zero, its slots answering nothing).
const WEAPON: u32 = AREA + 0x2_0000;
/// The `ActorValueInfo`s the table points at, 0x50 bytes each.
const INFOS: u32 = AREA + 0x1_0000;
/// The test's own functions (the creature's slots the hit asks).
const SLOTS: u32 = AREA + 0x3_f000;
const ANSWER_NONE: u32 = SLOTS;
const ANSWER_YES: u32 = SLOTS + 4;
const ANSWER_DEAD: u32 = SLOTS + 8;
const ANSWER_SELF: u32 = SLOTS + 12;

/// `BGSBodyPartData`'s 15 part slots, from `+0x34` (`bgsbodypartdata.rs`'s `part_slot`).
fn part_slot(index: u32) -> u32 {
    BODY_PART_DATA + 0x34 + 4 * index
}

/// A Gecko-like creature's body parts (`cActorValue` at `+0x63`): the torso none (-1), the head
/// the perception condition (0x19), the four legs their conditions (0x1b..0x1e), and a part whose
/// byte is outside the table (0x55: nothing the game's own data should hold, the index the 14:49
/// crash's read could have taken).
pub(crate) const PARTS: [(u32, i8); 7] = [
    (0, -1),
    (1, 0x19),
    (3, 0x1b),
    (4, 0x1c),
    (5, 0x1d),
    (6, 0x1e),
    (7, 0x55),
];

/// The hit effects asked for (`0088e8d0`).
pub(crate) static EFFECTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The game's memory for the creature: its pages mapped, the actor-value table filled (a mapped
/// `ActorValueInfo` for each of the 0x4d values) and the word past it the crash read (pi/2), the
/// creature, its process, its base form's body parts, its cell and the hit.
pub(crate) fn lay_out_gecko(e: &mut Engine) {
    e.mem.map(AREA, AREA_SIZE);
    // The exe's static data (its settings, flags and globals), as zero where nothing below sets
    // it: the game's start-up values are not loaded in the fixture.
    for page in (0x0100_0000u32..0x0128_0000).step_by(0x1000) {
        if !e.mem.is_mapped(page) {
            e.mem.map(page, 0x1000);
        }
    }
    for i in 0..crate::av_table::COUNT {
        let info = INFOS + 0x50 * i;
        e.mem.set_u32(crate::av_table::TABLE + 4 * i, info);
        e.mem.set_u32(info + 0x44, 0);
    }
    // What the 14:49 crash read: pi/2, a float, where an index past the table lands.
    e.mem
        .set_u32(crate::av_table::TABLE + 4 * 0x55, 0x3fc9_0fdb);
    e.mem.set_f64(0x0101_2060, 0.0);
    // The hit's visual effects (`0088e8d0`: the blood spray and decals, drawn with the C
    // runtime's maths on the exe's constants, which the fixture does not load): called, counted.
    e.register_double(0x0088_e8d0, |_, _| {
        EFFECTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ret::default()
    });
    // The platform's `GetLastError` (`00fdf094`; the C runtime's per-thread data asks it).
    e.register_double(0x00fd_f094, |_, _| 0u32.into_ret());
    // The platform's `GetTickCount` (the import at `00fdf060`): a clock in milliseconds.
    let mut tick = 1_000u32;
    e.register_double(0x00fd_f060, move |_, _| {
        tick += 16;
        tick.into_ret()
    });
    e.register_double(ANSWER_NONE, |_, _| Ret::default());
    e.register_double(ANSWER_YES, |_, _| 1u32.into_ret());
    e.register_double(ANSWER_DEAD, |_, _| 1u32.into_ret());
    e.register_double(ANSWER_SELF, |_, a| a[0].into_ret());
    for slot in 0..0x400u32 {
        e.mem.set_u32(GECKO_VTABLE + 4 * slot, ANSWER_NONE);
        e.mem.set_u32(PROCESS_VTABLE + 4 * slot, ANSWER_NONE);
        e.mem.set_u32(BASE_VTABLE + 4 * slot, ANSWER_NONE);
        e.mem.set_u32(OWNER_VTABLE + 4 * slot, ANSWER_NONE);
    }
    e.mem.set_u32(GECKO, GECKO_VTABLE);
    e.mem.set_u32(GECKO + rig_fixture::FORM_ID, GECKO_ID);
    e.mem.set_u32(GECKO + rig_fixture::PROCESS, GECKO_PROCESS);
    e.mem.set_u32(GECKO_PROCESS, PROCESS_VTABLE);
    // Its actor values (`+0xa4`): a creature's current value is `008d4870`, the game's.
    e.mem.set_u32(GECKO + 0xa4, OWNER_VTABLE);
    // Its magic target (`+0x94`): no effects on it.
    e.mem.set_u32(GECKO + 0x94, BASE_VTABLE);
    e.mem.set_u32(OWNER_VTABLE + 0x0c, 0x008d_4870);
    // Its base form (`TESObjectREFR` `+0x20`) and the base's body part data (slot `0x180`).
    e.mem.set_u32(GECKO + 0x20, BASE_FORM);
    e.mem.set_u32(BASE_FORM, BASE_VTABLE);
    // Its part at `+0x30` (the material the impact sounds ask), a vtable of its own.
    e.mem.set_u32(BASE_FORM + 0x30, BASE_VTABLE);
    // Its RTTI, as the compiler lays it out (the word before the vtable a complete object
    // locator, its class hierarchy one base array, single inheritance): a creature's class over
    // `TESForm`'s type descriptor (`011846e8`, the source type `Actor::DismemberLimbs`' cast
    // names); the cast's target (`01183a00`) is not among its bases.
    const RTTI: u32 = AREA + 0x3_e000;
    let (creature_td, form_td) = (RTTI, 0x0118_46e8u32);
    e.mem.write(creature_td + 8, b".?AVTESCreature@@\0");
    e.mem.write(form_td + 8, b".?AVTESForm@@\0");
    let bases = RTTI + 0x100;
    for (i, td) in [creature_td, form_td].into_iter().enumerate() {
        let b = RTTI + 0x200 + 0x20 * i as u32;
        e.mem.set_u32(b, td);
        e.mem.set_u32(b + 4, (1 - i) as u32);
        e.mem.set_i32(b + 0xc, -1);
        e.mem.set_u32(bases + 4 * i as u32, b);
    }
    let hierarchy = RTTI + 0x300;
    e.mem.set_u32(hierarchy + 8, 2);
    e.mem.set_u32(hierarchy + 0xc, bases);
    let locator = RTTI + 0x400;
    e.mem.set_u32(locator + 0xc, creature_td);
    e.mem.set_u32(locator + 0x10, hierarchy);
    e.mem.set_u32(BASE_VTABLE - 4, locator);
    e.register_double(SLOTS + 16, |_, _| BODY_PART_DATA.into_ret());
    e.mem.set_u32(BASE_VTABLE + 0x180, SLOTS + 16);
    for (k, &(slot, value)) in PARTS.iter().enumerate() {
        let part = BODY_PARTS + 0x100 * k as u32;
        e.mem.set_u8(part + 0x63, value as u8);
        e.mem.set_u8(part + 0x62, 20);
        e.mem.set_u32(part_slot(slot), part);
    }
    // Each actor's cached-values owner (`+0xa8`, eye level and radius): answering nothing.
    for who in [GECKO, PLAYER, SHOOTER] {
        e.mem.set_u32(who + 0xa8, BASE_VTABLE);
    }
    for who in [PLAYER, SHOOTER] {
        e.mem.set_u32(who, GECKO_VTABLE);
        e.mem.set_u32(who + rig_fixture::PROCESS, GECKO_PROCESS);
        e.mem.set_u32(who + 0xa4, OWNER_VTABLE);
        e.mem.set_u32(who + 0x20, BASE_FORM);
    }
    e.mem.set_u32(0x011d_ea3c, PLAYER);
    e.mem.set_u32(WEAPON, BASE_VTABLE);
    e.mem.set_u32(0x011c_a278, WEAPON);
    // The controls (`Controls::GetInstance`, `011f35cc`): the pad's rumble off.
    e.mem.set_u32(0x011f_35cc, AREA + 0x2_8000);
    // The combat manager (`011f1958`): no combat groups yet.
    e.mem.set_u32(0x011f_1958, AREA + 0x2_4000);
    // The sound listener (`011deb7c`'s object, its `+0xac` node: the impact sounds' distance).
    e.mem.set_u32(0x011d_eb7c, AREA + 0x2_1000);
    e.mem.set_u32(AREA + 0x2_1000 + 0xac, AREA + 0x2_3000);
    // In a loaded cell (`+0x1d0` its 3D: some; the parent cell attached).
    e.mem.set_u32(GECKO + 0x40, CELL);
    e.mem.set_u8(CELL + 0x26, 6);
    // Its 3D (`+0x1d0`): the root the fixture laid out for it (`Actor::Get3D`'s answer), the
    // creature's tree for the player and the biter, who have none of their own.
    e.register_double(SLOTS + 40, |e, a| {
        let root = e.call(0x0043_fcd0, &[a[0]]).u32();
        if root != 0 { root } else { CREATURE_TREE }.into_ret()
    });
    e.mem.set_u32(GECKO_VTABLE + 0x1d0, SLOTS + 40);
    // The damage (`+0x338`): lethal from the tenth pellet on (the game's answer, kept here).
    e.mem.set_u32(GECKO_VTABLE + 0x338, ANSWER_DEAD);
    e.register_double(ANSWER_DEAD, |_, _| {
        u32::from(PELLETS.load(std::sync::atomic::Ordering::Relaxed) >= 10).into_ret()
    });
    // A creature (`+0x100`: an actor the knockdowns and reactions take), its position (`+0x1f4`).
    e.mem.set_u32(GECKO_VTABLE + 0x100, ANSWER_YES);
    e.register_double(SLOTS + 20, |_, a| {
        (a[0] + rig_fixture::REFR_POSITION).into_ret()
    });
    e.mem.set_u32(GECKO_VTABLE + 0x1f4, SLOTS + 20);
    e.mem.set_f32(GECKO + rig_fixture::REFR_POSITION, 120.0);
    // Its process's actor values (`+0x39c`, what `008805f0` asks): health 40 less 4 a pellet,
    // the limbs' conditions 100.
    e.register_double(SLOTS + 24, |_, a| {
        let pellets = PELLETS.load(std::sync::atomic::Ordering::Relaxed) as f64;
        Ret {
            st0: if a[2] == 0x10 {
                40.0 - 4.0 * pellets
            } else {
                100.0
            },
            ..Ret::default()
        }
    });
    e.mem.set_u32(PROCESS_VTABLE + 0x39c, SLOTS + 24);
}

/// A Gecko-like tree: the mannequin's bones without hands (as a creature's skeleton has fewer
/// than a person's): New Physics, whose bodies include the hands, does not take it; the current
/// hit reactions (the 14:49 run's "flinch writing 11 of 19 bones") do.
fn gecko_like() -> Vec<rig_fixture::Node> {
    let all = rig_fixture::mannequin();
    let keep: Vec<usize> = (0..all.len())
        .filter(|&i| !all[i].name.ends_with(" Hand"))
        .collect();
    let mut tree: Vec<rig_fixture::Node> = keep
        .iter()
        .map(|&i| {
            let mut n = all[i].clone();
            n.parent = n.parent.and_then(|p| keep.iter().position(|&k| k == p));
            n
        })
        .collect();
    // Its 3D where the creature stands (`GECKO`'s reference, x 120).
    tree[0].t = [120.0, 0.0, 0.0];
    tree
}

/// Where the creature's tree is laid out (`Fixture::add_actor`).
const CREATURE_TREE: u32 = 0x7700_0000;

/// The pellets fired so far.
pub(crate) static PELLETS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The hit of a pellet on `part` of the creature.
pub(crate) fn pellet(e: &mut Engine, part: u32) {
    let hit = Ptr::<HitData>::new(HIT);
    for o in (0..0x64).step_by(4) {
        e.mem.set_u32(HIT + o, 0);
    }
    e.set(hit, HitData::pTarget, GECKO);
    e.set(hit, HitData::pAggressor, PLAYER);
    e.set(hit, HitData::pWeapon, WEAPON);
    e.set(hit, HitData::eDamageLimb, part);
    e.set(hit, HitData::fHealthDamage, 9.0f32.to_bits());
    e.set(hit, HitData::fTotalDamage, 9.0f32.to_bits());
    e.set(hit, HitData::fTargetedLimbDamage, 9.0f32.to_bits());
}

/// The platform under both threads, as the game's imports answer (`TlsGetValue` `00fdf044`,
/// `TlsSetValue` `00fdf04c`, `EnterCriticalSection` `00fdf05c`, `LeaveCriticalSection`
/// `00fdf100`): thread-local slots per OS thread, and one recursive critical section that
/// really excludes the other thread. Plain functions, so the AI thread's engine has them too.
pub(crate) mod platform {
    use engine::prelude::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::SeqCst};

    thread_local! {
        static SLOTS: RefCell<[u32; 64]> = const { RefCell::new([0; 64]) };
        static ME: u64 = {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            NEXT.fetch_add(1, SeqCst)
        };
    }
    static OWNER: AtomicU64 = AtomicU64::new(0);
    static DEPTH: AtomicU32 = AtomicU32::new(0);

    fn tls_get(_: &mut Engine, a: &[u32]) -> Ret {
        SLOTS.with(|s| s.borrow()[a[0] as usize % 64]).into_ret()
    }
    fn tls_set(_: &mut Engine, a: &[u32]) -> Ret {
        SLOTS.with(|s| s.borrow_mut()[a[0] as usize % 64] = a[1]);
        1u32.into_ret()
    }
    fn enter(_: &mut Engine, _: &[u32]) -> Ret {
        let me = ME.with(|m| *m);
        loop {
            if OWNER.load(SeqCst) == me {
                DEPTH.fetch_add(1, SeqCst);
                break;
            }
            if OWNER.compare_exchange(0, me, SeqCst, SeqCst).is_ok() {
                DEPTH.store(1, SeqCst);
                break;
            }
            std::thread::yield_now();
        }
        Ret::default()
    }
    fn leave(_: &mut Engine, _: &[u32]) -> Ret {
        if DEPTH.fetch_sub(1, SeqCst) == 1 {
            OWNER.store(0, SeqCst);
        }
        Ret::default()
    }

    /// The imports registered (before the AI thread's engine is made from this one).
    pub fn install(e: &mut Engine) {
        e.register(0x00fd_f044, tls_get);
        e.register(0x00fd_f04c, tls_set);
        e.register(0x00fd_f05c, enter);
        e.register(0x00fd_f100, leave);
    }

    /// This thread's slot `index` set (the thread's Havok memory router, as its start-up does).
    pub fn set_slot(index: u32, value: u32) {
        SLOTS.with(|s| s.borrow_mut()[index as usize % 64] = value);
    }
}

/// The Havok world both threads move their character proxies in, laid out in guest memory as the
/// game's code reads it: the world (its operation queue at `+0x88` with room for 64 records,
/// the pending counts `+0x8c`/`+0x90`, the critical-operation locks `+0x94`/`+0x98`, `m_isLocked`
/// `+0xbc`, the broad phase `+0x58` (its own lock off), the kd-tree manager `+0x5c`, the
/// dispatcher `+0x64`, the collision input `+0x78`), the Havok lock record (`012681d4`, locking
/// on), and for each proxy its shape phantom (a sphere of 0.3, the game's `hkpSphereShape`
/// `getAabbImpl` at its vtable's `+0x1c`) in that world. The broad phase's `updateAabbs` (slot
/// `0x28`) finds no new pairs and the kd-tree manager's slot `0x14` does nothing: the world's
/// other bodies are not in the fixture.
mod world {
    use engine::prelude::*;

    pub const AREA: u32 = 0x7600_0000;
    pub const WORLD: u32 = AREA;
    const QUEUE: u32 = AREA + 0x1000;
    const RECORDS: u32 = AREA + 0x2000;
    const BROAD_PHASE: u32 = AREA + 0x3000;
    const TREE: u32 = AREA + 0x3100;
    const DISPATCHER: u32 = AREA + 0x3200;
    const INPUT: u32 = AREA + 0x3300;
    const VTABLES: u32 = AREA + 0x4000;
    const SHAPE: u32 = AREA + 0x5000;
    const LOCK: u32 = AREA + 0x6000;
    const NOTHING_FN: u32 = 0x7200_0f00;
    /// The proxies: the NP actor's controller's (main thread) and an AI mover's (AI thread).
    pub const PROXIES: [u32; 2] = [AREA + 0x8000, AREA + 0x9000];
    const PHANTOMS: [u32; 2] = [AREA + 0xa000, AREA + 0xb000];
    /// The threads' memory routers (stack top at `+0`, allocator at `+0x10`) and their stacks.
    pub const ROUTERS: [u32; 2] = [AREA + 0xc000, AREA + 0xc100];
    const STACKS: [u32; 2] = [AREA + 0x1_0000, AREA + 0x2_0000];
    /// The router's TLS index (`01268108`) and the Havok lock's (`012681d8`).
    pub const ROUTER_TLS: u32 = 7;

    fn nothing(_: &mut Engine, _: &[u32]) -> Ret {
        Ret::default()
    }

    pub fn lay_out(e: &mut Engine, controller: u32) {
        e.mem.map(AREA, 0x4_0000);
        e.register(NOTHING_FN, nothing);
        for slot in 0..0x40u32 {
            e.mem.set_u32(VTABLES + 4 * slot, NOTHING_FN);
        }
        e.mem.set_u32(WORLD + 0x58, BROAD_PHASE);
        e.mem.set_u32(BROAD_PHASE, VTABLES);
        e.mem.set_u32(WORLD + 0x5c, TREE);
        e.mem.set_u32(TREE, VTABLES);
        e.mem.set_u32(WORLD + 0x64, DISPATCHER);
        e.mem.set_u32(WORLD + 0x78, INPUT);
        for lane in 0..4 {
            e.mem.set_f32(INPUT + 0x20 + 4 * lane, 1000.0);
            e.mem.set_f32(INPUT + 0x40 + 4 * lane, 10.0);
        }
        e.mem.set_f32(INPUT + 8, 0.05);
        e.mem.set_u32(WORLD + 0x33c, 4);
        // The operation queue: room for 64 records (no growth), its world.
        e.mem.set_u32(WORLD + 0x88, QUEUE);
        e.mem.set_u32(QUEUE, RECORDS);
        e.mem.set_u32(QUEUE + 4, 0);
        e.mem.set_u32(QUEUE + 8, 64 | 0x8000_0000);
        e.mem.set_u32(QUEUE + 0xc, WORLD);
        // The Havok lock (locking on), the TLS indices.
        e.mem.set_u32(0x0126_81d4, LOCK);
        e.mem.set_u32(LOCK + 8, 1);
        e.mem.set_u32(0x0126_81d8, 5);
        e.mem.set_u32(0x0126_8108, ROUTER_TLS);
        for k in 0..2 {
            e.mem.set_u32(ROUTERS[k], STACKS[k]);
            e.mem.set_u32(ROUTERS[k] + 0x10, VTABLES + 0x200);
            e.mem.set_u32(VTABLES + 0x200, VTABLES);
        }
        // The shape: a sphere (its `getAabbImpl`, the game's).
        e.mem.set_u32(SHAPE, VTABLES + 0x100);
        for slot in 0..0x20u32 {
            e.mem.set_u32(VTABLES + 0x100 + 4 * slot, NOTHING_FN);
        }
        e.mem.set_u32(VTABLES + 0x100 + 0x1c, 0x00cd_8100);
        e.mem.set_f32(SHAPE + 0x10, 0.3);
        for k in 0..2 {
            let (proxy, phantom) = (PROXIES[k], PHANTOMS[k]);
            e.mem.set_u32(proxy + 0x30, phantom);
            e.mem.set_f32(proxy + 0x58, 0.1);
            e.mem.set_u32(phantom + 8, WORLD);
            e.mem.set_u32(phantom + 0x10, SHAPE);
            // Its collidable's motion state (`+0x18`): its own, at `+0xb0`.
            e.mem.set_u32(phantom + 0x18, phantom + 0xb0);
            // Its transform (the motion state at `+0xb0`): the identity.
            for i in 0..3 {
                e.mem.set_f32(phantom + 0xb0 + 0x14 * i, 1.0);
            }
        }
        // The NP actor's controller: its Havok proxy (`+8`, its vtable's `UnMarkWrite` slot
        // `0x98` nothing in this build), its `bhkShapePhantom` (`+0x594`, the Havok phantom at
        // its `+8`).
        e.mem.set_u32(controller, VTABLES);
        e.mem.set_u32(controller + 8, PROXIES[0]);
        e.mem.set_u32(controller + 0x594, AREA + 0xd000);
        e.mem.set_u32(AREA + 0xd000 + 8, PHANTOMS[0]);
    }

    /// The NP actor's phantom's position (`+0xe0`, Havok units: where the controller's
    /// `SetPosition` chain put it) and its quantized box's least corner (`+0x30`).
    pub fn np_phantom(e: &Engine) -> ([f32; 3], [i32; 3]) {
        let p = PHANTOMS[0];
        (
            [0, 4, 8].map(|o| e.mem.f32(p + 0xe0 + o)),
            [0, 4, 8].map(|o| e.mem.i32(p + 0x30 + o)),
        )
    }

    /// The world's lock counts (`+0x94`, `+0x98`), its queue's records and `m_isLocked`.
    pub fn state(e: &Engine) -> (i32, i32, u32, i32) {
        (
            e.mem.u32(WORLD + 0x94) as i32,
            e.mem.u32(WORLD + 0x98) as i32,
            e.mem.u32(QUEUE + 4),
            e.mem.u32(WORLD + 0xbc) as i32,
        )
    }
}

/// A panic's message.
fn text_of(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<String>()
        .cloned()
        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Whether the AI thread's task runs now (what `008c7aa0` answers).
static AI_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The AI thread's engine, moved to its OS thread (the engine's view of the shared memory: its
/// functions are plain, none of the main engine's doubles).
struct Sent(Engine);
// SAFETY: a thread view (`Engine::thread_view`) holds no test double and no reference into the
// main engine; its memory is a view of the shared guest space. It is used by one thread at a
// time (moved to the AI thread, joined before the test ends).
unsafe impl Send for Sent {}

/// Patrick's 14:49 crash on c08805c1 (New Physics on): the first pellet on a Gecko while New
/// Physics' actor was stepping, then `HitMe` < `008d4870` < `00406d70` read pi/2 at
/// `[011d61c8 + 4 * index]` and faulted. Here the game's own `Actor::HitMe` (hooked as in play,
/// `knockdown::hit_me_hooked`, with its knockdown and hit-reaction decisions after it) takes 15
/// pellets on a creature with Gecko-like body parts, one of them with a `cActorValue` outside
/// the table (0x55), while New Physics' actor takes a hit and steps; the creature dies of the
/// tenth. Two threads: the main thread (the pellets, New Physics' write, its main-thread pass
/// that moves the reference through the controller's real `SetPosition` chain under the world's
/// lock) and an AI thread (each frame's task: an AI mover's `hkpCharacterProxy::setPosition`,
/// the game's chain to `hkpPhantom::updateBroadPhase`, on the same world's `+0x94`/`+0x98`)
/// while the main thread answers `008c7aa0` (an AI task running) truthfully.
///
/// Asserted: the actor-value table unchanged word for word (checked before and after every hit,
/// every write and every pass), `00406d70` never reading past it (nor an entry that is no
/// pointer) while the creature's limbs were read through it, the world's lock counts never
/// below 0 and back at 0 with no queued record after every frame, the NP actor kept and stepping,
/// the creature's hit reaction run, and no fault on either thread.
///
/// Stand-ins, the fixture having no game data loaded: the creature's slots answer nothing unless
/// set (its damage lethal from the tenth pellet, its actor values from its process's `+0x39c`),
/// the hit's blood and decals (`0088e8d0`) counted, not drawn; the platform's TLS and critical
/// section ([`platform`]); the broad phase finds no pairs ([`world`]).
#[test]
fn np_creature_hits_never_corrupt_the_actor_value_table() {
    use std::sync::atomic::{AtomicI32, AtomicU32, Ordering::SeqCst};
    use std::sync::{mpsc, Arc};
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut f = Fixture::new(
        rig_fixture::fnv_skeleton(),
        Ground {
            collision: Some(-1.0),
            navmesh: -9.0,
        },
    );
    f.make_character();
    lay_out_gecko(&mut f.e);
    f.add_actor(GECKO, GECKO_ID, gecko_like(), CREATURE_TREE);
    f.e.mem.set_u32(GECKO + rig_fixture::FORM_ID, GECKO_ID);
    f.game_update();
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    crate::av_table::clear();
    PELLETS.store(0, SeqCst);
    EFFECTS.store(0, SeqCst);
    let was = crate::gunplay::tuning();
    crate::gunplay::set_tuning(crate::gunplay::Tuning {
        new_physics: true,
        knockdowns: true,
        ..was
    });
    crate::knockdown::install(&mut f.e);
    // `008c7aa0`: whether an AI thread's task runs now (a plain function: both threads ask it).
    fn ai_running(_: &mut Engine, _: &[u32]) -> Ret {
        u32::from(AI_RUNNING.load(SeqCst)).into_ret()
    }
    f.e.register(0x008c_7aa0, ai_running);
    // The controller's `SetPosition` the game's (`005620e0` and on, translated), on the world.
    platform::install(&mut f.e);
    world::lay_out(&mut f.e, rig_fixture::CONTROLLER);
    f.e.register(
        0x0056_20e0,
        engine::entry!(
            0x0056_20e0,
            engine::units::fallout_shared::tesobjectrefr::bhk_character_controller_set_position(
                Ptr, Ptr
            )
        )
        .1,
    );
    // The game-to-Havok scale (`011c582c`, what `004a3e90` multiplies by): the fixture's.
    f.e.mem.set_f32(0x011c_582c, 1.0 / rig_fixture::HK);
    platform::set_slot(world::ROUTER_TLS, world::ROUTERS[0]);
    room_events::set_on(true);
    let _ = room_events::take();

    // The AI thread: each frame's task, an AI mover's proxy moved a little, the lock counts seen.
    let lowest = Arc::new(AtomicI32::new(0));
    let ai_moves = Arc::new(AtomicU32::new(0));
    let (go, task) = mpsc::channel::<bool>();
    let (done, finished) = mpsc::channel::<Result<(), String>>();
    let ai = Sent(f.e.thread_view());
    let (low, moves) = (lowest.clone(), ai_moves.clone());
    let handle = std::thread::spawn(move || {
        let mut ai = ai;
        platform::set_slot(world::ROUTER_TLS, world::ROUTERS[1]);
        let at = ai.0.mem.alloc(0x10);
        let mut x = 0.0f32;
        while let Ok(true) = task.recv() {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for _ in 0..8 {
                    x += 0.01;
                    for (i, v) in [x, 2.0, 0.5, 0.0].into_iter().enumerate() {
                        ai.0.mem.set_f32(at + 4 * i as u32, v);
                    }
                    ai.0.call(0x00ca_c920, &[world::PROXIES[1], at]);
                    moves.fetch_add(1, SeqCst);
                    let (a, b, _, _) = world::state(&ai.0);
                    low.fetch_min(a.min(b), SeqCst);
                }
            }));
            let r = r.map_err(|p| text_of(&p));
            let failed = r.is_err();
            let _ = done.send(r);
            if failed {
                break;
            }
        }
    });

    let mut faults = Vec::new();
    let mut frames_unsettled = Vec::new();
    let mut kept = true;
    let mut np_hit = false;
    let mut moved_from = f.reference();
    for frame in 0..240u32 {
        f.e.mem.set_f32(0x011d_ea30, 1.0 / 60.0);
        // The main thread's frame before the AI tasks: New Physics' hit, a pellet on the
        // creature (`HitMe` with no AI task running: the game's own, not queued).
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if frame == 4 {
                f.animate();
                np_hit = claim(&mut f.e, ACTOR);
                let chest = f.world("Bip01 Spine2").unwrap().1;
                crate::new_physics::hit(
                    &mut f.e,
                    ACTOR,
                    1.4 * crate::new_physics::model().mass,
                    [chest[0], chest[1] + 8.0, chest[2]],
                    [0.0, -1.0, 0.0],
                );
            }
            if frame >= 8 && frame % 15 == 8 && PELLETS.load(SeqCst) < 15 {
                let part = PARTS[PELLETS.load(SeqCst) as usize % PARTS.len()].0;
                pellet(&mut f.e, part);
                // `Actor::HitMe` as the game calls it: knockdown's hook, installed above.
                f.e.call(crate::knockdown::HIT_ME, &[GECKO, HIT, 0]);
                PELLETS.fetch_add(1, SeqCst);
            }
        }));
        if let Err(p) = r {
            faults.push(format!(
                "frame {frame}, main thread's hits: {}",
                text_of(&p)
            ));
            break;
        }
        // The AI threads' tasks start; the draw's set-up on the main thread meanwhile (New
        // Physics' write, the reactions').
        AI_RUNNING.store(true, SeqCst);
        go.send(true).unwrap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f.animate();
            crate::active_ragdoll::apply(&mut f.e);
        }));
        if let Err(p) = r {
            faults.push(format!(
                "frame {frame}, main thread's draw set-up: {}",
                text_of(&p)
            ));
            break;
        }
        // The AI tasks joined (the main thread's safe point).
        match finished.recv() {
            Ok(Ok(())) => {}
            Ok(Err(m)) => {
                faults.push(format!("frame {frame}, AI thread: {m}"));
                break;
            }
            Err(_) => {
                faults.push(format!("frame {frame}, AI thread gone"));
                break;
            }
        }
        AI_RUNNING.store(false, SeqCst);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::new_physics::main_thread(&mut f.e);
            crate::active_ragdoll::read_drawn(&mut f.e);
            crate::active_ragdoll::after_draw(&mut f.e);
            crate::active_ragdoll::unwrite(&mut f.e);
        }));
        if let Err(p) = r {
            faults.push(format!(
                "frame {frame}, main thread's pass: {}",
                text_of(&p)
            ));
            break;
        }
        let (a, b, queued, locked) = world::state(&f.e);
        lowest.fetch_min(a.min(b), SeqCst);
        if (a, b, queued, locked) != (0, 0, 0, 0) {
            frames_unsettled.push((frame, a, b, queued, locked));
        }
        if frame > 4 && !owns(ACTOR) {
            kept = false;
        }
    }
    let _ = go.send(false);
    handle.join().unwrap();
    let moved = sub(f.reference(), moved_from);
    moved_from = f.reference();
    let _ = moved_from;
    let lines = room_events::take();
    room_events::set_on(false);
    // Where the NP actor's moves put its phantom (game units), against its reference.
    let (phantom, quantized) = world::np_phantom(&f.e);
    let phantom_game = phantom.map(|v| v * rig_fixture::HK);
    let phantom_off = length(sub(
        [phantom_game[0], phantom_game[1], 0.0],
        [f.reference()[0], f.reference()[1], 0.0],
    ));
    let reactions = lines
        .iter()
        .filter(|l| l.contains("FF001323") && !l.contains("[np "))
        .count();
    // The bones each flinch wrote ("..., N of 18 bones; ...").
    let bones_written: u32 = lines
        .iter()
        .filter(|l| l.contains("Hit reaction: FF001323: flinch"))
        .filter_map(|l| {
            let at = l.find(" of 18 bones")?;
            l[..at].rsplit(' ').next()?.parse::<u32>().ok()
        })
        .sum();
    let np_took_the_creature = lines.iter().any(|l| l.contains("FF001323: [np take]"));
    let proxies = lines.iter().filter(|l| l.contains("[np proxy]")).count();
    let (reads, past, changes, checks) = (
        crate::av_table::reads(),
        crate::av_table::reads_past(),
        crate::av_table::changes(),
        crate::av_table::checks(),
    );
    eprintln!(
        "RESULT np creature hits: pellets {}, hit effects {}; 00406d70 reads {reads}, past the \
         table {past}; table changes {changes} in {checks} checks; lowest lock count {}, frames \
         not settled {frames_unsettled:?}; AI moves {}; New Physics took its actor {np_hit}, kept \
         {kept}, moved {:.1} units; the creature's knockdown and reaction lines {reactions}, its \
         flinches' bones {bones_written}, the NP phantom at {phantom_game:.1?} ({phantom_off:.2} \
         units off the reference across, quantized {quantized:?}), \
         [np proxy] lines {proxies}; faults {faults:#?}",
        PELLETS.load(SeqCst),
        EFFECTS.load(SeqCst),
        lowest.load(SeqCst),
        ai_moves.load(SeqCst),
        length(moved)
    );
    for l in lines.iter().filter(|l| l.contains("FF001323")).take(12) {
        eprintln!("  {l}");
    }
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    crate::gunplay::set_tuning(was);
    assert!(faults.is_empty(), "{faults:#?}");
    assert_eq!(PELLETS.load(SeqCst), 15);
    assert!(
        reads >= 10,
        "the creature's limbs were read through the table {reads} times"
    );
    assert_eq!(past, 0, "00406d70 read past the table");
    assert_eq!(changes, 0, "the actor-value table changed");
    assert!(checks >= 30 + 2 * 240, "the table checked {checks} times");
    assert!(lowest.load(SeqCst) >= 0, "a lock count went below 0");
    assert!(frames_unsettled.is_empty(), "{frames_unsettled:?}");
    assert!(ai_moves.load(SeqCst) >= 240 * 8);
    assert!(
        np_hit && kept,
        "New Physics took its actor {np_hit}, kept {kept}"
    );
    assert!(
        length(moved) >= 0.5,
        "New Physics' actor moved {:.2} units",
        length(moved)
    );
    assert!(
        reactions > 0,
        "no knockdown or reaction decision on the creature"
    );
    assert!(!np_took_the_creature, "New Physics took the creature");
    assert!(EFFECTS.load(SeqCst) > 0);
    assert!(
        phantom_off < 1.0 && length([phantom_game[0], phantom_game[1], 0.0]) > 0.5,
        "the NP actor's moves did not reach its phantom: {phantom_game:?}"
    );
    assert!(
        bones_written > 0,
        "the creature's hit reaction wrote no bone"
    );
}

/// The NPC's actor values (its owner's slot `+0xC`): Strength 6, damage threshold 2, health
/// 100, the rest 50.
const NPC_OWNER_VTABLE: u32 = AREA + 0x2_5000;
/// The biting creature (a Coyote's id) and its bite's damage (its slot `+0x444`).
const BITER_ID: u32 = 0xff00_1327;
const BITE: i32 = 12;

/// The fixture's NPC (`ACTOR`) made whole for the game's `HitMe`: its vtable's and its process's
/// unset slots answering nothing, its actor values ([`NPC_OWNER_VTABLE`]), its base form, cell
/// and magic target the creature's.
fn dress_npc(e: &mut Engine) {
    let vt = e.mem.u32(ACTOR);
    let process = e.mem.u32(ACTOR + rig_fixture::PROCESS);
    let pvt = e.mem.u32(process);
    for slot in 0..0x400u32 {
        for table in [vt, pvt] {
            if e.mem.u32(table + 4 * slot) == 0 {
                e.mem.set_u32(table + 4 * slot, ANSWER_NONE);
            }
        }
        e.mem.set_u32(NPC_OWNER_VTABLE + 4 * slot, ANSWER_NONE);
    }
    e.register_double(SLOTS + 28, |_, a| Ret {
        st0: match a[1] {
            5 => 6.0,
            0x4c => 2.0,
            0x10 => 100.0,
            _ => 50.0,
        },
        ..Ret::default()
    });
    e.mem.set_u32(NPC_OWNER_VTABLE + 0xc, SLOTS + 28);
    e.mem.set_u32(ACTOR + 0xa4, NPC_OWNER_VTABLE);
    e.mem.set_u32(ACTOR + 0x20, BASE_FORM);
    e.mem.set_u32(ACTOR + 0x40, CELL);
    e.mem.set_u32(ACTOR + 0x94, BASE_VTABLE);
    e.mem.set_u32(ACTOR + 0xa8, BASE_VTABLE);
    e.mem.set_u32(vt + 0x1d0, SLOTS + 40);
}

/// Patrick's 16:17 crash on 08ff8723 (New Physics on, NPCs turned hostile; a Gecko, a Coyote and
/// a Giant Rat fighting): `00406d70` < `008d4870` < `HitMe` < `Actor::CombatHit` < the attack
/// animation's hit < the task queue read `[011d62fc]` (index 77, one past the table) and faulted.
/// The game's own `HitData::ClearData` (`009b4dc0`: `eAttackSkill` 0x4d, none) and
/// `InitializeHitData_ov3` (`009b5170`, the melee hit: a creature with no weapon takes its
/// bite's damage, slot `+0x444`, and keeps that skill) make a Coyote's bite; the game's
/// `Actor::HitMe` through the hook (`knockdown::hit_me_hooked`) then lands it on an NPC and on a
/// creature, with New Physics on and off. `crate::hit_force` measured the swing with the hit's
/// skill through the creature's actor-value owner (`008d4870` < `00406d70`).
///
/// Asserted, each run: no fault; `00406d70` never read past the table and the table unchanged;
/// the swing measured with skill 0 (the knockdown line says "skill 0", or New Physics' hit line
/// for its actor); its momentum the formula's (`ragdoll_core::knock::transferred` of a swing of
/// 0 lb, reach 1, the Coyote's Strength, skill 0, against the target's damage threshold).
#[test]
fn np_creature_melee_hit_never_reads_past_the_actor_value_table() {
    use ragdoll_core::knock::{transferred, Push};
    use std::sync::atomic::Ordering::SeqCst;
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut failed = Vec::new();
    for np in [true, false] {
        for (who, target) in [("an NPC", ACTOR), ("a creature", GECKO)] {
            let mut f = Fixture::new(
                rig_fixture::fnv_skeleton(),
                Ground {
                    collision: Some(-1.0),
                    navmesh: -9.0,
                },
            );
            f.make_character();
            lay_out_gecko(&mut f.e);
            f.add_actor(GECKO, GECKO_ID, gecko_like(), CREATURE_TREE);
            f.game_update();
            dress_npc(&mut f.e);
            // What the 16:17 crash read one past the table: pi/2.
            f.e.mem
                .set_u32(crate::av_table::TABLE + 4 * 0x4d, 0x3fc9_0fdb);
            // The biter: a creature (`+0x21c`), its bite (`+0x444`), its own form id.
            f.e.mem.set_u32(GECKO_VTABLE + 0x21c, ANSWER_YES);
            f.e.register_double(SLOTS + 32, |_, _| (BITE as u32).into_ret());
            f.e.mem.set_u32(GECKO_VTABLE + 0x444, SLOTS + 32);
            f.e.mem.set_u32(SHOOTER + rig_fixture::FORM_ID, BITER_ID);
            // Its animation (`+0x1e4`): the attack's group in the upper-body section's word.
            f.e.register_double(SLOTS + 36, |_, _| (AREA + 0x2_6000).into_ret());
            f.e.mem.set_u32(GECKO_VTABLE + 0x1e4, SLOTS + 36);
            crate::active_ragdoll::clear();
            crate::knockdown::clear();
            crate::av_table::clear();
            PELLETS.store(0, SeqCst);
            let was = crate::gunplay::tuning();
            crate::gunplay::set_tuning(crate::gunplay::Tuning {
                new_physics: np,
                knockdowns: true,
                ..was
            });
            crate::knockdown::install(&mut f.e);
            room_events::set_on(true);
            let _ = room_events::take();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                f.animate();
                // The bite as the game makes it.
                f.e.call(0x009b_4dc0, &[HIT]);
                f.e.call(0x009b_5170, &[HIT, SHOOTER, target, 0, 0]);
                let skill = f.e.get(Ptr::<HitData>::new(HIT), HitData::eAttackSkill);
                // `Actor::HitMe` as the game calls it: knockdown's hook, installed above.
                f.e.call(crate::knockdown::HIT_ME, &[target, HIT, 0]);
                skill
            }));
            let lines = room_events::take();
            room_events::set_on(false);
            let (past, changes) = (crate::av_table::reads_past(), crate::av_table::changes());
            // The formula: the Coyote's Strength (its process's actor values: 100, taken as
            // 10), skill 0, against the target's damage threshold.
            let dt = if target == ACTOR { 2.0 } else { 100.0 };
            let want = transferred(
                Push::Swing {
                    weight: 0.0,
                    reach: 1.0,
                    strength: 100.0,
                    skill: 0.0,
                    power: false,
                },
                dt,
            );
            let id = if target == ACTOR {
                format!("{:08X}", rig_fixture::ACTOR_ID)
            } else {
                format!("{GECKO_ID:08X}")
            };
            let swing_line = lines
                .iter()
                .find(|l| l.contains(&id) && l.contains("swing ("))
                .cloned();
            let np_line = lines
                .iter()
                .find(|l| l.contains(&id) && l.contains("[np hit]"))
                .cloned();
            let momentum = lines
                .iter()
                .find(|l| l.contains("\"event\":\"hit\"") && l.contains(&id))
                .and_then(|l| {
                    let at = l.find("\"momentum\":")? + "\"momentum\":".len();
                    l[at..].split(',').next()?.parse::<f32>().ok()
                });
            eprintln!(
                "RESULT creature melee, New Physics {np}, on {who}: skill in the hit {:?}, \
                 00406d70 reads {} (past the table {past}), table changes {changes}, momentum \
                 {momentum:?} (the formula {want:.3}); {swing_line:?} {np_line:?}",
                r.as_ref().ok(),
                crate::av_table::reads(),
            );
            let tag = format!("New Physics {np}, {who}");
            match &r {
                Err(p) => failed.push(format!("{tag}: fault: {}", text_of(p))),
                Ok(skill) if *skill != 0x4d => failed.push(format!(
                    "{tag}: the bite's skill {skill:#x}, not ClearData's"
                )),
                _ => {}
            }
            if past != 0 || changes != 0 {
                failed.push(format!("{tag}: past the table {past}, changes {changes}"));
            }
            let np_has = np && target == ACTOR;
            if np_has {
                // New Physics' actor: its hit line carries the momentum.
                match np_line.as_deref().and_then(|l| {
                    let at = l.find("[np hit] ")? + "[np hit] ".len();
                    l[at..].split(' ').next()?.parse::<f32>().ok()
                }) {
                    Some(p) if (p - want * was.hit_force as f32 / 100.0).abs() < 0.05 => {}
                    other => failed.push(format!("{tag}: [np hit] {other:?}, want {want:.3}")),
                }
            } else {
                if !swing_line
                    .as_deref()
                    .is_some_and(|l| l.contains("skill 0)"))
                {
                    failed.push(format!("{tag}: no swing line with skill 0: {swing_line:?}"));
                }
                if !momentum.is_some_and(|m| (m - want).abs() < 1e-3) {
                    failed.push(format!("{tag}: momentum {momentum:?}, want {want:.4}"));
                }
            }
            crate::active_ragdoll::clear();
            crate::knockdown::clear();
            crate::gunplay::set_tuning(was);
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// What one run of [`bite_run`] left: every node's world of both trees, the NPC's ragdoll bodies'
/// velocities, both actors' knock states and reference positions, the actors the reactions
/// still had (none), the knockdown's decision lines, and the hit data's damage after the kill
/// impulse (the game's push reads it).
#[derive(Debug, PartialEq)]
struct BiteState {
    worlds: Vec<(u32, [u32; 12])>,
    bodies: Vec<[u32; 3]>,
    knock: [u8; 2],
    refs: [[u32; 3]; 2],
    reacting: Vec<u32>,
    decisions: usize,
    /// The knockdowns' switches into the game's `KnockExplosion` (their "knock state after the
    /// switch" lines).
    switched: usize,
    after_kill: [u32; 4],
}

/// Three bites of the Coyote's (the game's `ClearData` and `InitializeHitData_ov3`) on the NPC
/// and on the creature, the third lethal, then 30 frames, then the kill impulse (`008ae000`) of
/// the last bite. `fork`: none, the game's own `HitMe` and `008ae000` with no nv-rs hook (vanilla
/// FNV); else nv-rs's hooks installed as in play (`gunplay::install`), New Physics off and the
/// knockdowns and the ballistics as given, each frame the reactions' pass as the game's frame
/// runs it.
fn bite_run(fork: Option<(bool, bool)>) -> BiteState {
    use std::sync::atomic::Ordering::SeqCst;
    let mut f = Fixture::new(
        rig_fixture::fnv_skeleton(),
        Ground {
            collision: Some(-1.0),
            navmesh: -9.0,
        },
    );
    f.make_character();
    lay_out_gecko(&mut f.e);
    f.add_actor(GECKO, GECKO_ID, gecko_like(), CREATURE_TREE);
    f.game_update();
    dress_npc(&mut f.e);
    f.e.mem.set_u32(GECKO_VTABLE + 0x21c, ANSWER_YES);
    f.e.register_double(SLOTS + 32, |_, _| (BITE as u32).into_ret());
    f.e.mem.set_u32(GECKO_VTABLE + 0x444, SLOTS + 32);
    f.e.mem.set_u32(SHOOTER + rig_fixture::FORM_ID, BITER_ID);
    f.e.register_double(SLOTS + 36, |_, _| (AREA + 0x2_6000).into_ret());
    f.e.mem.set_u32(GECKO_VTABLE + 0x1e4, SLOTS + 36);
    // The character controller in a Havok world (what the game's `KnockExplosion` reads).
    platform::install(&mut f.e);
    world::lay_out(&mut f.e, rig_fixture::CONTROLLER);
    f.e.mem.set_f32(0x011c_582c, 1.0 / rig_fixture::HK);
    platform::set_slot(world::ROUTER_TLS, world::ROUTERS[0]);
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    let was = crate::gunplay::tuning();
    if let Some((knockdowns, ballistics)) = fork {
        crate::gunplay::set_tuning(crate::gunplay::Tuning {
            new_physics: false,
            knockdowns,
            ballistics,
            // Low enough that the bites stagger or knock down: the knockdowns' own decision.
            knockdown_threshold: 5,
            ..was
        });
        crate::gunplay::install(&mut f.e);
    }
    room_events::set_on(true);
    let _ = room_events::take();
    for bite in 0..3u32 {
        // The third bite is lethal (`+0x338`'s answer from the tenth "pellet").
        PELLETS.store(if bite == 2 { 10 } else { 0 }, SeqCst);
        for target in [ACTOR, GECKO] {
            f.animate();
            f.e.call(0x009b_4dc0, &[HIT]);
            f.e.call(0x009b_5170, &[HIT, SHOOTER, target, 0, 0]);
            // `Actor::HitMe` as the game calls it: nv-rs's hook with `fork`, the game's own without.
            f.e.call(crate::knockdown::HIT_ME, &[target, HIT, 0]);
        }
        for _ in 0..10 {
            f.e.mem.set_f32(0x011d_ea30, 1.0 / 60.0);
            f.animate();
            if fork.is_some() {
                crate::new_physics::main_thread(&mut f.e);
                crate::active_ragdoll::apply(&mut f.e);
                crate::active_ragdoll::read_drawn(&mut f.e);
                crate::active_ragdoll::after_draw(&mut f.e);
                crate::active_ragdoll::unwrite(&mut f.e);
            }
        }
    }
    // The kill impulse of the last bite (the game's push of the killed body): the biter's
    // attack point (its process's `+0x460`) at its 3D's origin.
    f.e.register_double(SLOTS + 44, |_, _| (AREA + 0x2_7000).into_ret());
    f.e.mem.set_u32(PROCESS_VTABLE + 0x460, SLOTS + 44);
    f.e.call(0x008a_e000, &[SHOOTER, HIT]);
    let lines = room_events::take();
    room_events::set_on(false);
    let mut worlds = Vec::new();
    let trees: Vec<u32> =
        f.at.iter()
            .copied()
            .chain(f.others.iter().flat_map(|o| o.2.clone()))
            .collect();
    for n in trees {
        let mut w = [0u32; 12];
        for (i, o) in (0..9)
            .map(|i| rig_fixture::WORLD_ROTATION + 4 * i)
            .chain((0..3).map(|i| rig_fixture::WORLD_TRANSLATION + 4 * i))
            .enumerate()
        {
            w[i] = f.e.mem.u32(n + o);
        }
        worlds.push((n, w));
    }
    let bodies = (0..f.body_nodes().len())
        .map(|k| f.body_velocity(k).map(f32::to_bits))
        .collect();
    let knock_of = |e: &Engine, actor: u32| {
        let process = e.mem.u32(actor + rig_fixture::PROCESS);
        e.mem.u8(process + rig_fixture::KNOCK_STATE)
    };
    let ref_of = |e: &Engine, actor: u32| {
        [0, 4, 8].map(|o| e.mem.u32(actor + rig_fixture::REFR_POSITION + o))
    };
    let state = BiteState {
        worlds,
        bodies,
        knock: [knock_of(&f.e, ACTOR), knock_of(&f.e, GECKO)],
        refs: [ref_of(&f.e, ACTOR), ref_of(&f.e, GECKO)],
        reacting: crate::active_ragdoll::reacting_actors(),
        decisions: lines
            .iter()
            .filter(|l| l.contains("\"event\":\"hit\""))
            .count(),
        switched: lines
            .iter()
            .filter(|l| l.contains("knock state after the switch"))
            .count(),
        after_kill: [0x14, 0x18, 0x1c, 0x20].map(|o| f.e.mem.u32(HIT + o)),
    };
    crate::active_ragdoll::clear();
    crate::knockdown::clear();
    crate::gunplay::set_tuning(was);
    state
}

/// New Physics off is vanilla FNV's hits (Patrick, 2026-11-18). The same three bites of a
/// creature on an NPC and on a creature, the third lethal, then its kill impulse ([`bite_run`]):
/// - with the knockdowns and the ballistics off, nv-rs's hooks leave everything as the game's
///   own path does: every node's world bit for bit, the bodies' velocities, the knock states, the
///   references, the hit data after the kill impulse; no actor the reactions have; no knockdown
///   decision;
/// - with the knockdowns alone, the knockdowns decide on each living actor's bite (5: the third
///   kills the creature) and switch the NPC into the game's `KnockExplosion` on each of its three
///   (the creature's game refuses: a stagger), and nothing else differs: no bone written (the
///   worlds, bodies and the rest the vanilla run's);
/// - with the ballistics alone (they act on the projectiles, none here), all as vanilla.
#[test]
fn np_off_hits_are_vanilla_fnvs() {
    let _one = crate::active_ragdoll::tests::SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let vanilla = bite_run(None);
    let mut failed = Vec::new();
    for (name, knockdowns, ballistics, decisions) in [
        ("knockdowns and ballistics off", false, false, 0usize),
        ("knockdowns alone", true, false, 5),
        ("ballistics alone", false, true, 0),
    ] {
        let fork = bite_run(Some((knockdowns, ballistics)));
        let differ: Vec<u32> = fork
            .worlds
            .iter()
            .zip(&vanilla.worlds)
            .filter(|(a, b)| a != b)
            .map(|(a, _)| a.0)
            .collect();
        eprintln!(
            "RESULT New Physics off, {name}: nodes off the vanilla run {} of {}, bodies same {}, \
             knock states {:?} (vanilla {:?}), references same {}, reacting {:?}, knockdown \
             decisions {}, switches into the game's knock-down {}, the hit after the kill \
             impulse same {}",
            differ.len(),
            fork.worlds.len(),
            fork.bodies == vanilla.bodies,
            fork.knock,
            vanilla.knock,
            fork.refs == vanilla.refs,
            fork.reacting,
            fork.decisions,
            fork.switched,
            fork.after_kill == vanilla.after_kill
        );
        // The knockdowns' own switch (the game's `KnockExplosion`) only with them on: the NPC's
        // three bites (the creature's game refuses, a stagger, then it is dead).
        let switches = if knockdowns { 3 } else { 0 };
        if fork.switched != switches {
            failed.push(format!(
                "{name}: {} switches, want {switches}",
                fork.switched
            ));
        }
        if !differ.is_empty() {
            failed.push(format!(
                "{name}: {} nodes off the vanilla pose",
                differ.len()
            ));
        }
        if fork.bodies != vanilla.bodies {
            failed.push(format!("{name}: the bodies' velocities differ"));
        }
        if fork.knock != vanilla.knock || fork.refs != vanilla.refs {
            failed.push(format!("{name}: knock states or references differ"));
        }
        if !fork.reacting.is_empty() {
            failed.push(format!("{name}: the reactions have {:x?}", fork.reacting));
        }
        if fork.decisions != decisions {
            failed.push(format!(
                "{name}: {} knockdown decisions, want {decisions}",
                fork.decisions
            ));
        }
        if fork.after_kill != vanilla.after_kill {
            failed.push(format!("{name}: the kill impulse differs"));
        }
    }
    assert_eq!((vanilla.decisions, vanilla.switched), (0, 0));
    assert!(vanilla.reacting.is_empty());
    assert!(failed.is_empty(), "{failed:#?}");
}
