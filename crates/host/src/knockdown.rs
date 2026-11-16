//! Knockdowns of living actors (Patrick, 2026-11-14: Skyrim-style), on the ragdoll hit force
//! ([`crate::hit_force`]). Part of the weighty combat row; `[nv-rs Gunplay] bKnockdowns`,
//! `iKnockdownThreshold` (both on the Settings menu's Gameplay page) and `bKnockdownCompanions`.
//! **Not 1:1 with FalloutNV.exe.** Never the player.
//!
//! **When.** Over `Actor::HitMe` (`0089a760`, every hit: projectiles, swings, blasts; on the main
//! thread, the task queue's hits when they come back), after the game applied the hit: for a
//! target that is alive, not the player, not a companion (unless `bKnockdownCompanions=1`), not
//! already knocked down (its process's knock state `+0x13c`).
//!
//! **Physics** (Patrick, 2026-11-14: momentum transfer, not a score). The hit puts a momentum
//! into the body ([`transferred`], N s), added to the actor's stack ([`stack`]): every hit that
//! lands (each landed pellet of a shotgun, each round of a burst, each swing) adds its momentum,
//! and the stack fades by half every `iImpactStacking` milliseconds (Settings > Gameplay "Impact
//! Stacking", 0.1 to 1 s); its direction is the momentum-weighted sum of the hits' directions.
//! The decisions and the launch use the stack; a knockdown empties it. The body's speed from it
//! is that momentum over the body's mass ([`body_mass`]: the ragdoll's own Havok masses once measured, else by body and size), times
//! `iHitForce` percent, at most [`MAX_LAUNCH`]. What topples the body ([`topple_speed`]) is the
//! speed that lifts its centre of mass over the edge of its stance, times a recovery factor (a
//! standing body steps to catch itself), times `iKnockdownThreshold` percent. At or past it: a
//! knockdown, the body launched at that speed; at [`STAGGER_SHARE`] of it: a stagger.
//!
//! - At or past the resistance: a **knockdown**. The game's own blast knock-down
//!   (`MiddleHighProcess::KnockExplosion`, `0091fee0`) puts the actor into its ragdoll: it leaves
//!   its furniture, knock state 2, its bones go limp. Its own push is kept tiny
//!   ([`KNOCK_STRENGTH`]): `TESHavokUtilities::AddExplosionImpulse` (`0062b660`) adds to each bone's
//!   velocity its own direction from the push point times the strength (a speed in Havok units, no
//!   mass or falloff in it), and it runs before the ragdoll is fully set up.
//! - **The launch** ([`frame`]'s launches; Patrick, 2026-11-14: bodies dropped straight down): the
//!   frame after the ragdoll is limp (knock state 2), every bone gets the same velocity added along
//!   the hit (the hit data's direction `+0x44`; for a swing without one, from the attacker; for a
//!   blast, from its centre), lifted by [`LIFT`]: the momentum times [`LAUNCH_PER_POINT`] game units
//!   a second and `iHitForce` percent, [`MIN_LAUNCH`] to [`MAX_LAUNCH`] (about 2 to 10 m/s),
//!   turned into Havok's units by the game's own scale (`011b1864`, [`hk_scale`]). The next frame
//!   checks the bones kept at least [`LAUNCH_KEPT`] of it, and gives it again (at most
//!   [`LAUNCH_TRIES`]) if the ragdoll's set-up took it back. The 274ad9c build held every bone to
//!   12 *Havok* units a second (about 1.2 m/s) for 1.5 s, which took the launch and even the fall
//!   away: the limits are now in game units, for outliers only.
//! - **The get-up** ([`hold_get_up`]; Patrick: knocked-down actors flew up and shook when getting
//!   up, upside down against a wall): the game gets an actor up on a timer (`UpdateKnockState`,
//!   `00920150`: state 1 a moment after the ragdoll is limp, then the get-up idle), whether the
//!   body is still or lying. Its states 1 and 3 now wait until every bone is slower than
//!   [`STILL_SPEED`] for [`STILL_FRAMES`] frames and the body lies (its bones within
//!   [`LYING_HEIGHT`] in height), at most [`HOLD_MAX`]; then, on the main thread, the actor and
//!   its character controller are set where the ragdoll ended up, with no velocity: its pelvis
//!   (`Bip01 Pelvis`) on the ground under it ([`ground_under`]: the navmesh floor right under it,
//!   else the lowest bone less its radius), moved only when the capsule does not fit there, to
//!   the nearest nudge (16 to 64 units) clear of walls and statics (the navmesh's line of sight)
//!   and of other actors' capsules ([`fit_spot`]); else left at the body. (a5644a3 moved it to a
//!   navmesh point up to 200 units away, next to other actors and 20 units over the floor, and
//!   the physics threw them apart.) For [`WATCH`] after that ([`watch_get_up`]) its controller's
//!   stored push and the pushes other characters give it are zeroed for [`HOLD_PUSH`], its rising
//!   speed and its bones' are held to [`MAX_RISE`], and its position, velocity, overlaps and any
//!   depenetration are logged every [`WATCH_STEP`].
//! - **Once:** an actor knocked down in the last [`KNOCK_COOLDOWN`] or got up in the last
//!   [`UP_COOLDOWN`] is not knocked again, the game's own blast knock-down
//!   ([`crate::hit_force::KNOCK_EXPLOSION`]) adds no second push to an actor knocked here a moment
//!   ago ([`recently_knocked`]), and a push stored in the character controller is
//!   cleared before the ragdoll takes over. While it flies ([`FLIGHT`]) only a bone faster than
//!   [`MAX_BONE_SPEED`] or spinning faster than [`MAX_BONE_SPIN`] is held to it; no bone is touched
//!   otherwise, so the ragdoll can come to rest and sleep. While it lies, the knockback a hit on it
//!   stores in its character controller is cleared ([`clear_stored_push`]); and every frame each
//!   followed actor is checked to still be the same reference ([`live`]), all of it dropped on a
//!   load, a new game or the main menu ([`clear`]).
//!   Every launch, clamp, guard and get-up decision is a `Knockdown:` log line.
//! - At or past [`STAGGER_SHARE`] of it: a **stagger**: the game's own hit reaction only (which
//!   `Actor::HitMe` plays), logged; no push of any kind. Only a knockdown moves the body (Patrick,
//!   2026-11-15: rapid hits below topple each shoved the standing actor harder, by the stack, and
//!   the shoves added up in its controller and slid or launched it).
//!
//! **Falls** ([`frame`]): the game's fall damage runs from the character controller, which a body
//! in its ragdoll does not use. So each knocked-down actor is followed: its highest point and,
//! when it is down (knock state 1), getting up, or after [`FOLLOW_FOR`], where it lies. A drop past
//! the game's `fJumpFallHeightMin` hurts it with the game's formula (`(drop - min)^exponent *
//! mult`, the settings `Actor::Move` reads) through `Actor` slot `0x338` with the attacker, so a
//! kill is the attacker's (the player's), and each leg's condition takes the game's landing roll
//! (`iJumpFallLimbDamageChance` percent, damage times `fJumpFallLimbDamageMult`, slot `0x3ac`).
//!
//! **For a later leg-loss fall:** [`knock_down`] puts any living actor down from a point and a
//! direction with a momentum, the fall followed the same way.
//!
//! Game time is never touched; the kill camera is not involved.

use crate::hit_force::{shot_of, Shot};
use engine::prelude::*;
/// The pure parts (the bodies, the momentum transfer, the topple speed, the stack's fading, the
/// launch) live in `crates/ragdoll-core`, shared with `tools/ragdoll-lab`.
pub use ragdoll_core::knock::*;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The game's own knock-down (`KnockExplosion`) only puts the body into its ragdoll: its push is
/// this small. The launch is the knockdowns' own: the body's speed from the transferred momentum
/// ([`transferred`] / [`body_mass`]), lifted by [`LIFT`].
pub const KNOCK_STRENGTH: f32 = 2.0;
/// The launch is given again (at most this many times) when the next frame shows less than this
/// share of it left (the ragdoll's own start took it back).
pub const LAUNCH_TRIES: u8 = 2;
pub const LAUNCH_KEPT: f32 = 0.4;
pub const ORIGIN_BACK: f32 = 400.0;
/// No second knock-down this soon after one, nor this soon after getting up.
pub const KNOCK_COOLDOWN: Duration = Duration::from_millis(2500);
pub const UP_COOLDOWN: Duration = Duration::from_millis(1500);
/// A bone's speed (game units a second, turned into Havok's by the game's scale, [`hk_scale`]) and
/// spin (radians a second) at most while it flies (outliers only: the launch and a fall stay
/// well below), and how long that is held.
pub const MAX_BONE_SPEED: f32 = 1400.0;
pub const MAX_BONE_SPIN: f32 = 20.0;
pub const FLIGHT: Duration = Duration::from_millis(1500);
/// The get-up waits ([`hold_get_up`]) until every bone is slower than [`STILL_SPEED`] for
/// [`STILL_FRAMES`] frames in a row and the body lies (its bones within [`LYING_HEIGHT`] of each
/// other in height), at most [`HOLD_MAX`]; then the actor stands where the body lies
/// ([`fit_spot`]).
pub const HOLD_MAX: Duration = Duration::from_secs(HOLD_SECONDS as u64);
/// The get-up's spot: a capsule of [`CAPSULE_RADIUS`] (times the actor's scale) and
/// [`CAPSULE_HEIGHT`] fits at the ragdoll's pelvis, or at the nearest nudge ([`NUDGES`], eight
/// ways each, at most [`MAX_NUDGE`]) where it does (a navmesh point within [`NUDGE_SNAP`] of it);
/// the ground: the navmesh within [`GROUND_REACH`] across and [`GROUND_BELOW`] under the lowest
/// bone, else the lowest bone less [`BONE_RADIUS`].
pub const CAPSULE_RADIUS: f32 = 22.0;
pub const CAPSULE_HEIGHT: f32 = 120.0;
pub const NUDGES: [f32; 4] = [16.0, 32.0, 48.0, 64.0];
pub const MAX_NUDGE: f32 = 64.0;
pub const NUDGE_SNAP: f32 = 12.0;
pub const GROUND_REACH: f32 = 24.0;
pub const GROUND_BELOW: f32 = 40.0;
pub const BONE_RADIUS: f32 = 6.0;
/// After the get-up starts: watched (and logged every [`WATCH_STEP`]) for [`WATCH`], the
/// controller's pushes zeroed for [`HOLD_PUSH`], rising held to [`MAX_RISE`] game units a second.
pub const WATCH: Duration = Duration::from_secs(2);
pub const WATCH_STEP: Duration = Duration::from_millis(200);
pub const HOLD_PUSH: Duration = Duration::from_millis(600);
pub const MAX_RISE: f32 = 60.0;
pub const FOLLOW_FOR: Duration = Duration::from_secs(12);

// --- The game's words. -------------------------------------------------------------------------

/// `Actor::HitMe`.
pub const HIT_ME: u32 = 0x0089_a760;
/// The player, the task queue's test (true on an AI thread: the hit is queued, not applied).
const PLAYER: u32 = 0x011d_ea3c;
const AI_THREAD: u32 = 0x008c_7aa0;
/// An actor's process (`+0x68`), its knock state (`+0x13c`), its life state (`+0x108`: 1 dying,
/// 2 dead), `bPlayerTeammate` (`+0x18d`), its base form (`+0x20`) and position (virtual `+0x1f4`).
const PROCESS: u32 = 0x68;
const KNOCK_STATE: u32 = 0x13c;
/// A process's level (`+0x28`: 0 high, 1 middle high; a lower one has no knock state) and the
/// highest a knock-down takes; a reference's form id and position; `HitData`'s projectile or
/// explosion.
const PROCESS_LEVEL: u32 = 0x28;
const MIDDLE_HIGH: u8 = 1;
const FORM_ID: u32 = 0x0c;
const REFR_POSITION: u32 = 0x30;
const HIT_SOURCE: u32 = 0x08;
/// `Actor::CanKnockDown`, and what it asks (module documentation).
const CAN_KNOCK_DOWN: u32 = 0x0088_45a0;
const BASE_FLAG_TEST: u32 = 0x0046_1580;
const LIFE_STATE: u32 = 0x108;
const TEAMMATE: u32 = 0x18d;
const BASE_FORM: u32 = 0x20;
const SLOT_POSITION: u32 = 0x1f4;
/// `TESNPC::pFormRace`, a creature's type getter, `TESObjectREFR::GetScale`.
const NPC_RACE: u32 = 0x110;
const CREATURE_TYPE: u32 = 0x0059_f3a0;
const GET_SCALE: u32 = 0x0056_7400;
/// Form types: NPC, creature.
const NPC: u8 = 0x2a;
const CREA: u8 = 0x2b;
/// `HitData`: the aggressor, the target, the impact point and direction.
const HIT_AGGRESSOR: u32 = 0x00;
const HIT_TARGET: u32 = 0x04;
const HIT_POINT: u32 = 0x38;
const HIT_DIRECTION: u32 = 0x44;
const HIT_WEAPON: u32 = 0x30;
/// Actor values: health, damage threshold, the legs' conditions.
const AV_HEALTH: u32 = 0x10;
const AV_DAMAGE_THRESHOLD: u32 = 0x4c;
const LEGS: [u32; 2] = [0x1d, 0x1e];
/// `Actor` slots: the health and fatigue damage (with the attacker), a limb's damage.
const SLOT_DAMAGE: u32 = 0x338;
const SLOT_LIMB: u32 = 0x3ac;
/// The settings `Actor::Move` reads for fall damage, the landing's limb roll, and their getters.
const FALL_MIN: u32 = 0x011c_ff94;
const FALL_MULT: u32 = 0x011d_1410;
const FALL_EXPONENT: u32 = 0x011d_0d20;
const LIMB_MULT: u32 = 0x011d_108c;
const LIMB_CHANCE: u32 = 0x011d_10c8;
const SETTING_FLOAT: u32 = 0x0040_3e20;
const SETTING_INT: u32 = 0x0043_d4d0;
const RANDOM: u32 = 0x0048_7f50;

fn actor_value(e: &mut Engine, actor: u32, index: u32) -> f32 {
    let v = e.vcall(actor + 0xa4, 0xc, &args![index]).f64() as f32;
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

fn position(e: &mut Engine, actor: u32) -> Option<[f32; 3]> {
    let at = e.vcall(actor, SLOT_POSITION, &args![]).u32();
    (at != 0).then(|| [0, 4, 8].map(|o| e.mem.f32(at + o)))
}

fn setting_f(e: &mut Engine, setting: u32) -> f32 {
    let at = e.call(SETTING_FLOAT, &args![setting]).u32();
    if at == 0 {
        0.0
    } else {
        e.mem.f32(at)
    }
}

/// The body of `actor` (its base form: a creature's type, an NPC's race by name).
fn body_of(e: &mut Engine, actor: u32) -> Body {
    let base = e.mem.u32(actor + BASE_FORM);
    if base == 0 {
        return Body::Human;
    }
    match e.mem.u8(base + 4) {
        CREA => Body::Creature(e.call(CREATURE_TYPE, &args![base]).u8() as i8),
        NPC => {
            let race = e.mem.u32(base + NPC_RACE);
            let name = if race == 0 {
                Vec::new()
            } else {
                jip::commands::the_name(e, race).to_ascii_lowercase()
            };
            let text = String::from_utf8_lossy(&name);
            if text.contains("mutant") || text.contains("nightkin") {
                Body::SuperMutant
            } else {
                Body::Human
            }
        }
        _ => Body::Human,
    }
}

/// The actors knocked down and followed for their fall: (actor, attacker, highest z, since).
/// (actor, its form id, attacker, highest z, since).
type Followed = (u32, u32, u32, f32, Instant);
static DOWN: Mutex<Vec<Followed>> = Mutex::new(Vec::new());

/// Registers the hook (from `crate::gunplay::install`).
pub fn install(e: &mut Engine) {
    use engine::units::fallout_ai::actor_p3::actor_hit_me;
    let t = crate::gunplay::tuning();
    eprintln!(
        "Knockdown: bKnockdowns={} iKnockdownThreshold={} iHitForce={} bKnockdownCompanions={} \
         (each decision on a living character is logged as Knockdown: lines)",
        u8::from(t.knockdowns),
        t.knockdown_threshold,
        t.hit_force,
        u8::from(t.knockdown_companions)
    );
    e.register(UPDATE_KNOCK_STATE, |e, a| {
        if crate::gunplay::is_on() && a[0] != 0 && hold_get_up(e, a[0], a[1]) {
            return Ret::default();
        }
        let (_, update) = engine::entry!(
            0x00920150,
            engine::units::fallout_ai::middlehighprocess::middle_high_process_update_knock_state(
                Ptr<engine::units::fallout_ai::middlehighprocess::MiddleHighProcess>,
                u32
            )
        );
        update(e, a)
    });
    e.register(HIT_ME, |e, a| {
        let queued = e.call(AI_THREAD, &[]).bool();
        actor_hit_me(e, Ptr::new(a[0]), Ptr::new(a[1]), a[2] as u8);
        if !queued {
            after_hit(e, a[0], a[1]);
        }
        Ret::default()
    });
}

/// After the game applied the hit `hit` to `target`: a knockdown or a stagger (module
/// documentation). Each decision on a living character is logged ([`note`]).
fn after_hit(e: &mut Engine, target: u32, hit: u32) {
    let t = crate::gunplay::tuning();
    if !crate::gunplay::is_on() || !t.knockdowns || hit == 0 || target == 0 {
        return;
    }
    let player = e.global::<u32>(PLAYER);
    if target == player || e.mem.u32(hit + HIT_TARGET) != target {
        return;
    }
    if !e.vcall(target, 0x100, &args![]).bool() {
        return; // not a character
    }
    let who = describe(e, target);
    // A round's continuation back on the body it passed through: the same round, already counted
    // (`crate::ballistics::passed_through`).
    if crate::ballistics::passed_through(e.mem.u32(hit + HIT_SOURCE), target) {
        note(format!(
            "{who}: the round that passed through it again: no second hit"
        ));
        return;
    }
    if e.mem.u8(target + TEAMMATE) != 0 && !t.knockdown_companions {
        note(format!(
            "{who}: a companion (bKnockdownCompanions=0), left standing"
        ));
        return;
    }
    let process = e.mem.u32(target + PROCESS);
    if process == 0 {
        return;
    }
    let level = e.mem.u8(process + PROCESS_LEVEL);
    let state = e.mem.u8(process + KNOCK_STATE) as i8;
    let life = e.mem.u32(target + LIFE_STATE);
    let health = actor_value(e, target, AV_HEALTH);
    if level > MIDDLE_HIGH || state != 0 || matches!(life, 1 | 2) || health <= 0.0 {
        note(format!(
            "{who}: not knocked (process level {level}, knock state {state}, life state {life}, \
             health {health:.0})"
        ));
        return;
    }
    let aggressor = e.mem.u32(hit + HIT_AGGRESSOR);
    if crate::active_ragdoll::falling(target) {
        note(format!(
            "{who}: going over into its ragdoll: this hit waits for it"
        ));
        return;
    }
    let Measured {
        push,
        dt,
        p,
        point,
        direction,
    } = measure_hit(e, target, hit);
    let half_life = t.impact_stacking.max(1) as f32 / 1000.0;
    let stacked = stack(target, p, direction, half_life);
    let hit_direction = direction;
    let (total, direction) = (stacked.total, stacked.direction);
    let scale = e.call(GET_SCALE, &args![target]).f64() as f32;
    let body = body_of(e, target);
    let (mass, measured) = mass_of(e, target, body, scale);
    let speed = total / mass * t.hit_force.max(0) as f32 / 100.0;
    let topple = topple_speed(body, scale, t.knockdown_threshold);
    // Pain (this hit's own push over the topple speed, added to what earlier hits left)
    // escalates the reaction: sustained fire staggers, then knocks down
    // (`ragdoll_core::condition`; the actor's fatigue and hurt limbs are kept with it).
    let own = p / mass * t.hit_force.max(0) as f32 / 100.0;
    let stage = crate::active_ragdoll::condition_hit(e, target, own / topple.max(0.1), point);
    let decision = ragdoll_core::condition::escalate(reaction(speed, topple), stage);
    if crate::room_events::on() {
        use crate::room_events::{num, text};
        crate::room_events::record(
            "hit",
            &format!(
                "\"actor\":\"{:08X}\",\"momentum\":{},\"stacked\":{},\"speed\":{},\"topple\":{},\"mass\":{},\"pain\":{},\"decision\":{},\"point\":[{},{},{}],\"direction\":[{},{},{}]",
                e.mem.u32(target + FORM_ID),
                num(p),
                num(total),
                num(speed),
                num(topple),
                num(mass),
                text(&format!("{stage:?}")),
                text(&format!("{decision:?}")),
                num(point[0]),
                num(point[1]),
                num(point[2]),
                num(hit_direction[0]),
                num(hit_direction[1]),
                num(hit_direction[2])
            ),
        );
    }
    note(format!(
        "{who}: {} {p:.1} N s (DT {dt:.0}); stack {:.1} N s left of earlier hits + this = {total:.1} \
         N s over {} hits (half-life {half_life:.2} s), along ({:.2}, {:.2}, {:.2}) -> {speed:.2} \
         m/s on {mass:.0} kg ({body:?}, scale {scale:.2}, {}) vs topple {topple:.2} m/s (threshold \
         {}%, Hit Force {}%) -> {decision:?}",
        describe_push(push),
        stacked.before,
        stacked.hits,
        direction[0],
        direction[1],
        direction[2],
        if measured {
            "its ragdoll's mass"
        } else {
            "by body and size"
        },
        t.knockdown_threshold,
        t.hit_force
    ));
    match decision {
        Reaction::Knockdown => {
            if speed > MAX_LAUNCH {
                note(format!(
                    "{who}: clamp: launch {speed:.1} m/s held to {MAX_LAUNCH} m/s"
                ));
            }
            reset_stack(target);
            note(format!("{who}: stack reset on the knockdown"));
            // Stage 3: the posed body goes over first (the flinch's kick on top), then falls into
            // its ragdoll; without the layer, the hard switch as before.
            crate::active_ragdoll::hit(e, target, p, mass, point, hit_direction, false);
            // Not knocked down now (just knocked down or up, the game refuses): no fall pose
            // (it would go over and stand up again), a stagger (Patrick's 10:47 clip on 454ebb2).
            if let Err(why) = can_knock_now(e, target) {
                note(format!(
                    "{who}: no knockdown now ({why}): a stagger instead"
                ));
                crate::active_ragdoll::interrupt_attack(e, target, "staggered");
                crate::active_ragdoll::push(e, target, p, mass, hit_direction, aggressor);
                return;
            }
            // No `StopAttack` here: the ragdoll ends the attack, and nothing is changed in the
            // process before the game's `CanKnockDown` reads it.
            if !crate::active_ragdoll::fall(e, target, aggressor, point, direction, speed) {
                knock_down(e, target, aggressor, point, direction, speed)
            }
        }
        // No shove: only a knockdown moves the body. A stagger is the game's own hit reaction,
        // which `Actor::HitMe` has already played; pushing a standing actor's character controller
        // on every hit below topple (the stack's speed, so each rapid hit pushed harder) added up
        // and slid or launched it while it stood (Patrick, 2026-11-15).
        Reaction::Stagger => {
            crate::active_ragdoll::hit(e, target, p, mass, point, hit_direction, false);
            crate::active_ragdoll::interrupt_attack(e, target, "staggered");
            crate::active_ragdoll::push(e, target, p, mass, hit_direction, aggressor);
            note(format!(
            "{who}: stagger (the game's hit reaction only, no push): this hit {:.2} m/s ({p:.1} N s), \
             the stack {speed:.2} m/s ({total:.1} N s over {} hits) vs topple {topple:.2} m/s",
            p / mass * t.hit_force.max(0) as f32 / 100.0,
            stacked.hits
        ));
        }
        // Below the stagger too: the physics flinch, per hit (`crate::active_ragdoll`).
        Reaction::None => {
            crate::active_ragdoll::hit(e, target, p, mass, point, hit_direction, true);
            crate::active_ragdoll::push(e, target, p, mass, hit_direction, aggressor);
        }
    }
}

/// One hit as the knockdowns read it: what pushed, the target's damage threshold that counted, the
/// momentum it transferred (N s), its impact point and direction.
#[derive(Clone, Copy, Debug)]
pub struct Measured {
    pub push: Push,
    pub dt: f32,
    pub p: f32,
    pub point: [f32; 3],
    pub direction: [f32; 3],
}

/// The hit `hit` on `target` measured ([`Measured`]; module documentation).
pub fn measure_hit(e: &mut Engine, target: u32, hit: u32) -> Measured {
    let aggressor = e.mem.u32(hit + HIT_AGGRESSOR);
    let weapon = e.mem.u32(hit + HIT_WEAPON);
    let stats = crate::gunplay::effective_stats(e, aggressor, weapon);
    let shot = shot_of(e, hit, stats.as_ref());
    let source = e.mem.u32(hit + HIT_SOURCE);
    let push = match shot {
        Shot::Explosion { damage } => Push::Blast { damage },
        Shot::Melee {
            damage: _,
            weight,
            reach,
            strength,
            skill,
            power,
        } => Push::Swing {
            weight,
            reach,
            strength,
            skill,
            power,
        },
        Shot::Bullet { .. } | Shot::Gun { .. } => {
            match crate::ballistics::bullet_round(e, source)
                .or_else(|| crate::ballistics::weapon_round(e, weapon, aggressor))
            {
                Some(round) => Push::Round(round),
                None => Push::Damage {
                    damage: stats.map_or(e.mem.f32(hit + HIT_HEALTH_DAMAGE), |s| s.damage),
                },
            }
        }
    };
    // A blast's damage already has the threshold taken; it pushes away from its centre.
    let (dt, (point, direction)) = match push {
        Push::Blast { .. } => (0.0, blast(e, hit, target)),
        _ => (
            actor_value(e, target, AV_DAMAGE_THRESHOLD),
            impact(e, hit, target, aggressor),
        ),
    };
    Measured {
        push,
        dt,
        p: transferred(push, dt),
        point,
        direction,
    }
}

fn describe_push(push: Push) -> String {
    match push {
        Push::Round(r) => format!(
            "{} {:.1} g at {:.0} m/s ({:.2} N s, {:.0}% left in the body, x{BULLET_FEEL}):",
            if r.pellet { "pellet" } else { "round" },
            r.mass * 1000.0,
            r.speed,
            r.momentum(),
            r.share * 100.0
        ),
        Push::Swing {
            weight,
            strength,
            power,
            ..
        } => format!(
            "swing ({weight:.0} lb, Strength {strength:.0}{}):",
            if power { ", power attack" } else { "" }
        ),
        Push::Blast { damage } => format!("blast ({damage:.0} damage here):"),
        Push::Damage { damage } => format!("hit of no known calibre ({damage:.0} damage):"),
    }
}

/// A living actor's stacked impacts: the momentum left (N s), its momentum-weighted direction
/// (the sum of each hit's momentum times its direction, decayed the same), the hits in it, the
/// last hit.
#[derive(Clone, Copy, Debug)]
struct Stack {
    actor: u32,
    total: f32,
    vector: [f32; 3],
    hits: u32,
    at: Instant,
}

static STACKS: Mutex<Vec<Stack>> = Mutex::new(Vec::new());

/// The hit of `p` along `direction` added to `actor`'s stack (module documentation).
fn stack(actor: u32, p: f32, direction: [f32; 3], half_life: f32) -> Stacked {
    let mut list = STACKS.lock().unwrap_or_else(|p| p.into_inner());
    // Stacks faded out (ten half-lives) are dropped.
    list.retain(|s| s.at.elapsed().as_secs_f32() < half_life * 10.0);
    let old = list
        .iter()
        .position(|s| s.actor == actor)
        .map(|i| list.remove(i));
    let (total, vector, hits, seconds) = old.map_or((0.0, [0.0; 3], 0, 0.0), |s| {
        (s.total, s.vector, s.hits, s.at.elapsed().as_secs_f32())
    });
    let (stacked, vector) = add_hit(total, vector, hits, seconds, half_life, p, direction);
    list.push(Stack {
        actor,
        total: stacked.total,
        vector,
        hits: stacked.hits,
        at: Instant::now(),
    });
    stacked
}

/// The momentum `actor`'s stack holds now (N s), decayed.
pub fn stacked_now(actor: u32) -> f32 {
    let half_life = crate::gunplay::tuning().impact_stacking.max(1) as f32 / 1000.0;
    STACKS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|s| s.actor == actor)
        .map_or(0.0, |s| {
            decayed(s.total, s.at.elapsed().as_secs_f32(), half_life)
        })
}

fn reset_stack(actor: u32) {
    STACKS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .retain(|s| s.actor != actor);
}

/// The ragdoll masses measured (base form -> kg at its scale then, and that scale).
static MASSES: Mutex<Vec<(u32, f32, f32)>> = Mutex::new(Vec::new());

/// The body's mass: its kind's measured ragdoll mass (Havok's own, scaled by the size cubed), else
/// by body and size ([`body_mass`]); and whether it was measured.
fn mass_of(e: &Engine, actor: u32, body: Body, scale: f32) -> (f32, bool) {
    let base = e.mem.u32(actor + BASE_FORM);
    let measured = MASSES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|m| m.0 == base)
        .map(|m| m.1 * (sane_scale(scale) / sane_scale(m.2)).powi(3));
    match measured {
        Some(m) if m.is_finite() && m > 1.0 => (m, true),
        _ => (body_mass(body, scale), false),
    }
}

/// The ragdoll's Havok masses summed (`hkpMotion::getMass`, `00c86430`, of each rigid body's
/// motion at `+0xE0`), once its bones are dynamic (a keyframed bone weighs 0): kept for its base
/// form when every bone has a mass and the sum is within a fifth to five times the figure's.
fn measure(e: &mut Engine, actor: u32, body: Body, scale: f32) {
    let base = e.mem.u32(actor + BASE_FORM);
    if base == 0 {
        return;
    }
    let bones = bodies(e, actor);
    let mut sum = 0.0f32;
    for &(_, body) in &bones {
        let m = e.call(GET_MASS, &args![body + MOTION]).f64() as f32;
        if !(m.is_finite() && m > 0.0) {
            return;
        }
        sum += m;
    }
    let expected = body_mass(body, scale);
    let fits = !bones.is_empty() && sum > expected / 5.0 && sum < expected * 5.0;
    note(format!(
        "{}: ragdoll mass {sum:.0} kg over {} bones (by body and size {expected:.0} kg): {}",
        describe(e, actor),
        bones.len(),
        if fits {
            "used from now on for this kind"
        } else {
            "not used (out of range)"
        }
    ));
    if fits {
        let mut list = MASSES.lock().unwrap_or_else(|p| p.into_inner());
        list.retain(|m| m.0 != base);
        list.push((base, sum, sane_scale(scale)));
    }
}

/// `actor`'s state for the room's scripted runs (`crate::room_script`), as JSON fields: where it
/// stands (game units), the process's knock state (0 standing, else knocked or getting up), its
/// life state, health, and whether it is going over into its ragdoll or getting up.
pub fn state_fields(e: &mut Engine, actor: u32) -> String {
    use crate::room_events::num;
    let at = read3(e, actor + REFR_POSITION);
    let process = e.mem.u32(actor + PROCESS);
    let knock = if process == 0 {
        0
    } else {
        e.mem.u8(process + KNOCK_STATE) as i8
    };
    let life = e.mem.u32(actor + LIFE_STATE);
    let health = actor_value(e, actor, AV_HEALTH);
    format!(
        "\"position\":[{},{},{}],\"knock_state\":{knock},\"life_state\":{life},\"health\":{},\"falling\":{},\"getting_up\":{}",
        num(at[0]),
        num(at[1]),
        num(at[2]),
        num(health),
        crate::active_ragdoll::falling(actor),
        in_get_up(actor)
    )
}

/// An actor for the log: its form id and its base's name.
fn describe(e: &Engine, actor: u32) -> String {
    let base = e.mem.u32(actor + BASE_FORM);
    let name = if base == 0 {
        String::new()
    } else {
        String::from_utf8_lossy(&jip::commands::the_name(e, base)).into_owned()
    };
    format!("Knockdown: {:08X} {name}", e.mem.u32(actor + FORM_ID))
}

/// The log lines of the decisions, at most [`NOTES_PER_SECOND`] a second (the rest counted).
const NOTES_PER_SECOND: u32 = 20;
static NOTES: Mutex<(Option<Instant>, u32, u32)> = Mutex::new((None, 0, 0));

fn note(line: String) {
    crate::room_events::record(
        "knockdown_note",
        &format!("\"text\":{}", crate::room_events::text(&line)),
    );
    let mut n = NOTES.lock().unwrap_or_else(|p| p.into_inner());
    if n.0.map_or(true, |t| t.elapsed().as_secs() >= 1) {
        if n.2 > 0 {
            eprintln!("Knockdown: {} more decisions not logged", n.2);
        }
        *n = (Some(Instant::now()), 0, 0);
    }
    if n.1 < NOTES_PER_SECOND {
        n.1 += 1;
        eprintln!("{line}");
    } else {
        n.2 += 1;
    }
}

/// A blast's push: from its centre (the explosion reference's position) through the target's
/// chest.
fn blast(e: &mut Engine, hit: u32, target: u32) -> ([f32; 3], [f32; 3]) {
    let at = position(e, target).unwrap_or([0.0; 3]);
    let chest = [at[0], at[1], at[2] + 60.0];
    let source = e.mem.u32(hit + HIT_SOURCE);
    let centre = if source == 0 {
        None
    } else {
        Some([0, 4, 8].map(|o| e.mem.f32(source + REFR_POSITION + o)))
    };
    let d = centre
        .filter(|c| c.iter().all(|v| v.is_finite()))
        .map(|c| [chest[0] - c[0], chest[1] - c[1], chest[2] - c[2]])
        .unwrap_or([0.0, 0.0, 1.0]);
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if l > 1e-3 {
        (chest, d.map(|v| v / l))
    } else {
        (chest, [0.0, 0.0, 1.0])
    }
}

/// The hit's impact point and unit direction: the hit data's when it has them, else from the
/// attacker through the target's chest.
fn impact(e: &mut Engine, hit: u32, target: u32, aggressor: u32) -> ([f32; 3], [f32; 3]) {
    let point = [0, 4, 8].map(|o| e.mem.f32(hit + HIT_POINT + o));
    let way = [0, 4, 8].map(|o| e.mem.f32(hit + HIT_DIRECTION + o));
    let length = (way[0] * way[0] + way[1] * way[1] + way[2] * way[2]).sqrt();
    let point_ok = point.iter().all(|v| v.is_finite()) && point.iter().any(|v| *v != 0.0);
    if point_ok && length.is_finite() && length > 1e-3 {
        return (point, way.map(|v| v / length));
    }
    let at = position(e, target).unwrap_or([0.0; 3]);
    let chest = [at[0], at[1], at[2] + 60.0];
    let from = if aggressor == 0 {
        None
    } else {
        position(e, aggressor)
    };
    let d = from.map_or([0.0, 1.0, 0.0], |f| {
        let d = [at[0] - f[0], at[1] - f[1], 0.0];
        let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if l > 1e-3 {
            [d[0] / l, d[1] / l, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        }
    });
    (chest, d)
}

/// Whether a knocked-down body may get up: still for long enough and lying, or held too long.
pub fn may_get_up(still_frames: u32, height: f32, held: Duration) -> bool {
    (still_frames >= STILL_FRAMES && height <= LYING_HEIGHT) || held >= HOLD_MAX
}

/// A velocity held to `most` (its length), and whether it was.
pub fn clamp_speed(v: [f32; 3], most: f32) -> ([f32; 3], bool) {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l.is_nan() {
        return ([0.0; 3], true);
    }
    if l <= most {
        return (v, false);
    }
    (v.map(|x| x * most / l), true)
}

/// Whether `actor` would be knocked down now ([`knock_down`]'s own refusals, asked before a fall
/// pose begins): a process at the middle-high level or above, not the player, not within the
/// cooldowns after a knock-down or a get-up, and the game's `CanKnockDown`. `Err` says why not.
pub fn can_knock_now(e: &mut Engine, actor: u32) -> Result<(), String> {
    let process = e.mem.u32(actor + PROCESS);
    if process == 0 {
        return Err("no process".into());
    }
    if actor == e.global::<u32>(PLAYER) {
        return Err("the player".into());
    }
    if e.mem.u8(process + PROCESS_LEVEL) > MIDDLE_HIGH {
        return Err("a low process (no KnockExplosion)".into());
    }
    if let Some(why) = cooling_down(actor) {
        return Err(why);
    }
    if !e.call(CAN_KNOCK_DOWN, &args![actor]).bool() {
        return Err("the game's CanKnockDown refuses".into());
    }
    Ok(())
}

/// Knocks the living `actor` down into its ragdoll, pushed from `point` along `direction` with
/// the momentum `m` (module documentation), and follows its fall; `aggressor` is credited with a
/// fall's kill. The entry for any knock-down (a later leg-loss fall).
pub fn knock_down(
    e: &mut Engine,
    actor: u32,
    aggressor: u32,
    point: [f32; 3],
    direction: [f32; 3],
    body_speed: f32,
) {
    use engine::units::fallout_ai::middlehighprocess::middle_high_process_knock_explosion;
    let process = e.mem.u32(actor + PROCESS);
    if process == 0 || actor == e.global::<u32>(PLAYER) {
        crate::active_ragdoll::fall_failed(actor);
        return;
    }
    if e.mem.u8(process + PROCESS_LEVEL) > MIDDLE_HIGH {
        // KnockExplosion is MiddleHighProcess's: a lower process has none.
        note(format!(
            "{}: not knocked down: a low process (no KnockExplosion)",
            describe(e, actor)
        ));
        crate::active_ragdoll::fall_failed(actor);
        return;
    }
    let strength = KNOCK_STRENGTH;
    let speed = launch_speed(body_speed);
    let origin = [0, 1, 2].map(|i| point[i] - direction[i] * ORIGIN_BACK);
    let stood = position(e, actor);
    let start = stood.map_or(origin[2], |p| p[2]);
    let who = describe(e, actor);
    let id = e.mem.u32(actor + FORM_ID);
    if let Some(why) = cooling_down(actor) {
        note(format!("{who}: duplicate guard: {why}, not knocked again"));
        crate::active_ragdoll::fall_failed(actor);
        return;
    }
    if !e.call(CAN_KNOCK_DOWN, &args![actor]).bool() {
        // The game's own test refuses: say which of its questions did.
        let dead = e.vcall(actor, 0x22c, &args![1u32]).bool();
        let slot_4b4 = e.vcall(actor, 0x4b4, &args![]).bool();
        let base = e.mem.u32(actor + BASE_FORM);
        let no_knockdowns = base != 0
            && e.call(BASE_FLAG_TEST, &args![base + 0x30, 0x0400_0000u32])
                .bool();
        let action = e.vcall(process, 0x3e4, &args![]).i32();
        let other = e.vcall(actor, 0x1e4, &args![]).u32();
        let other_says = other != 0 && e.mem.u16(other + 0x122) != 0xff;
        note(format!(
            "{who}: the game's CanKnockDown refuses (dead {dead}, slot 0x4b4 {slot_4b4}, base \
             flag 0x4000000 {no_knockdowns}, process state {action} (9 or 17 refuse), slot 0x1e4 \
             refuses {other_says})"
        ));
        crate::active_ragdoll::fall_failed(actor);
        return;
    }
    let ragdoll = e.vcall(actor, 0x38c, &args![]).bool();
    // While the game's task queue is in use (its AI threads mid-frame: at the draw's set-up, where
    // the stage 3 fall used to call this) `KnockExplosion` only queues the request and returns
    // with knock state 0, and the queued replay met the duplicate guard: no knockdown at all
    // (779cd5b, Patrick, 2026-11-15). The switch is made on the main thread's own update instead.
    if e.call(AI_THREAD, &[]).bool() {
        knock_down_later(actor, id, aggressor, point, direction, body_speed, 0, &who);
        return;
    }
    // A push stored in the character controller (a script's, the game's knockback) would keep
    // moving it under the ragdoll.
    let _ = clear_stored_push(e, actor);
    KNOCKED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push((actor, Instant::now()));
    middle_high_process_knock_explosion(
        e,
        Ptr::new(process),
        actor,
        origin[0].to_bits(),
        origin[1].to_bits(),
        origin[2].to_bits(),
        strength,
    );
    let state = e.mem.u8(process + KNOCK_STATE) as i8;
    note(format!(
        "{who}: knock state after the switch: {state}{}",
        if state == 2 {
            " (in its ragdoll)"
        } else if !ragdoll {
            " (no ragdoll for this actor: the game only pushed it)"
        } else {
            " (the switch did not happen)"
        }
    ));
    if state == 0 {
        // Not down: nothing of a knockdown is kept (no cooldown, no get-up that would block its
        // reactions), and a ragdoll actor is tried again, once, the hard way next frame.
        KNOCKED
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|k| k.0 != actor);
        if ragdoll {
            let attempt = take_attempt(actor);
            if attempt < HARD_RETRIES {
                note(format!(
                    "{who}: knock state 0 after KnockExplosion: tried again next frame (the hard \
                     knock-down, attempt {})",
                    attempt + 2
                ));
                knock_down_later(
                    actor,
                    id,
                    aggressor,
                    point,
                    direction,
                    body_speed,
                    attempt + 1,
                    &who,
                );
            } else {
                note(format!(
                    "{who}: knock state still 0 after {} tries: left standing",
                    attempt + 1
                ));
                crate::active_ragdoll::fall_failed(actor);
            }
        }
        return;
    }
    if state == 2 {
        let scale = e.call(GET_SCALE, &args![actor]).f64() as f32;
        let body = body_of(e, actor);
        let known = MASSES
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .any(|m| m.0 == e.mem.u32(actor + BASE_FORM));
        if !known {
            measure(e, actor, body, scale);
        }
    }
    let launch = launch_direction(direction);
    // Given right away, where the game gives its own blast's push: KnockExplosion has just set the
    // bones moving under physics (`bhkWorld::SetMotion`), knock state 2.
    let given = if state == 2 {
        give_launch(e, actor, launch, speed)
    } else {
        0
    };
    note(format!(
        "{who}: impulse applied: {given} bones +{speed:.0} units/s ({:.1} m/s) along ({:.2}, {:.2}, \
         {:.2}) right after KnockExplosion (knock state {state})",
        speed / 70.0,
        launch[0],
        launch[1],
        launch[2]
    ));
    LAUNCHES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(Launch {
            actor,
            id,
            direction: launch,
            speed,
            frames: 0,
            tries: 0,
        });
    {
        let mut ups = GET_UPS.lock().unwrap_or_else(|p| p.into_inner());
        ups.retain(|g| g.actor != actor);
        ups.push(GetUp {
            actor,
            id,
            start: stood.unwrap_or(point),
            since: Instant::now(),
            lying_since: None,
            still_frames: 0,
            held_said: false,
            allowed: false,
            placed: None,
            next_log: 0.0,
            fall_dir: launch,
            braced: false,
        });
    }
    note(format!(
        "{who}: KnockExplosion strength {strength:.0} from ({:.0}, {:.0}, {:.0}): {} (knock state \
         now {state})",
        origin[0],
        origin[1],
        origin[2],
        if ragdoll {
            "ragdoll"
        } else {
            "no ragdoll for this actor (slot 0x38c): the game only pushes it"
        }
    ));
    let mut down = DOWN.lock().unwrap_or_else(|p| p.into_inner());
    down.retain(|d| d.0 != actor);
    down.push((actor, id, aggressor, start, Instant::now()));
}

/// A knock-down waiting for the main thread's own update (the task queue in use, or a retry).
#[derive(Clone, Copy, Debug)]
struct Pending {
    actor: u32,
    id: u32,
    aggressor: u32,
    point: [f32; 3],
    direction: [f32; 3],
    speed: f32,
    attempt: u8,
    since: Instant,
}

static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());
/// The retries a knock-down that left knock state 0 gets, and how long one waits for the task
/// queue to be free before it is given up.
const HARD_RETRIES: u8 = 1;
const PENDING_FOR: Duration = Duration::from_millis(600);
/// The attempt a retried knock-down is at (taken by the knock-down that runs).
static ATTEMPTS: Mutex<Vec<(u32, u8)>> = Mutex::new(Vec::new());

/// `actor`'s knock-down done on the main thread's next update ([`frame`]).
#[allow(clippy::too_many_arguments)]
pub fn knock_down_later(
    actor: u32,
    id: u32,
    aggressor: u32,
    point: [f32; 3],
    direction: [f32; 3],
    speed: f32,
    attempt: u8,
    who: &str,
) {
    let mut list = PENDING.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(p) = list.iter_mut().find(|p| p.actor == actor) {
        p.attempt = p.attempt.max(attempt);
        return;
    }
    list.push(Pending {
        actor,
        id,
        aggressor,
        point,
        direction,
        speed,
        attempt,
        since: Instant::now(),
    });
    drop(list);
    if attempt == 0 {
        note(format!(
            "{who}: the game's task queue is in use (its AI threads mid-frame): the knock-down \
             waits for the main thread's update"
        ));
    }
}

/// A stage 3 fall's knock-down, made at the main thread's next update ([`frame`]): never at the
/// draw's set-up, where the game's task queue is in use (`crate::active_ragdoll`'s fall).
pub fn knock_down_next_update(
    actor: u32,
    id: u32,
    aggressor: u32,
    point: [f32; 3],
    direction: [f32; 3],
    speed: f32,
) {
    let mut list = PENDING.lock().unwrap_or_else(|p| p.into_inner());
    if list.iter().any(|p| p.actor == actor) {
        return;
    }
    list.push(Pending {
        actor,
        id,
        aggressor,
        point,
        direction,
        speed,
        attempt: 0,
        since: Instant::now(),
    });
}

fn take_attempt(actor: u32) -> u8 {
    let mut list = ATTEMPTS.lock().unwrap_or_else(|p| p.into_inner());
    let a = list.iter().find(|a| a.0 == actor).map_or(0, |a| a.1);
    list.retain(|a| a.0 != actor);
    a
}

/// The knock-downs waiting ([`knock_down_later`]) done now, while the task queue is free.
fn pending_knock_downs(e: &mut Engine) {
    let list: Vec<Pending> =
        std::mem::take(&mut *PENDING.lock().unwrap_or_else(|p| p.into_inner()));
    for p in list {
        if !live(e, p.actor, p.id) {
            continue; // gone (a load, an unload): nothing of it is touched
        }
        let who = describe(e, p.actor);
        if e.call(AI_THREAD, &[]).bool() {
            if p.since.elapsed() < PENDING_FOR {
                PENDING.lock().unwrap_or_else(|p| p.into_inner()).push(p);
            } else {
                note(format!(
                    "{who}: the task queue stayed in use for {} ms: the knock-down is given up",
                    PENDING_FOR.as_millis()
                ));
                crate::active_ragdoll::fall_failed(p.actor);
            }
            continue;
        }
        ATTEMPTS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push((p.actor, p.attempt));
        knock_down(e, p.actor, p.aggressor, p.point, p.direction, p.speed);
        take_attempt(p.actor);
    }
}

/// Once a frame (from the player's look hook): each followed actor's highest point, and when it
/// is down, getting up or followed long enough, its fall's damage (module documentation).
pub fn frame(e: &mut Engine) {
    pending_knock_downs(e);
    let followed: Vec<Followed> = DOWN.lock().unwrap_or_else(|p| p.into_inner()).clone();
    launches(e);
    get_ups(e);
    if followed.is_empty() {
        return;
    }
    let mut keep = Vec::new();
    for (actor, id, aggressor, top, since) in followed {
        if !live(e, actor, id) {
            continue;
        }
        let process = e.mem.u32(actor + PROCESS);
        let Some(at) = position(e, actor) else {
            continue;
        };
        if process == 0 || matches!(e.mem.u32(actor + LIFE_STATE), 1 | 2) {
            continue;
        }
        let top = top.max(at[2]);
        let state = e.mem.u8(process + KNOCK_STATE) as i8;
        let settled = matches!(state, 0 | 1 | 3 | 5 | 6) || since.elapsed() > FOLLOW_FOR;
        if settled && since.elapsed() > Duration::from_millis(400) {
            land(e, actor, aggressor, top - at[2]);
            UP.lock()
                .unwrap_or_else(|p| p.into_inner())
                .push((actor, Instant::now()));
        } else {
            keep.push((actor, id, aggressor, top, since));
        }
    }
    *DOWN.lock().unwrap_or_else(|p| p.into_inner()) = keep;
}

/// The actors knocked down here and when, and those that got up again and when.
static KNOCKED: Mutex<Vec<(u32, Instant)>> = Mutex::new(Vec::new());
static UP: Mutex<Vec<(u32, Instant)>> = Mutex::new(Vec::new());

/// Why `actor` is not knocked down again now, if it isn't.
fn cooling_down(actor: u32) -> Option<String> {
    let mut knocked = KNOCKED.lock().unwrap_or_else(|p| p.into_inner());
    knocked.retain(|k| k.1.elapsed() < KNOCK_COOLDOWN);
    if let Some(k) = knocked.iter().find(|k| k.0 == actor) {
        return Some(format!("knocked down {} ms ago", k.1.elapsed().as_millis()));
    }
    let mut up = UP.lock().unwrap_or_else(|p| p.into_inner());
    up.retain(|u| u.1.elapsed() < UP_COOLDOWN);
    up.iter()
        .find(|u| u.0 == actor)
        .map(|u| format!("got up {} ms ago", u.1.elapsed().as_millis()))
}

/// Whether `actor` was knocked down here in the last moment (the game's own blast knock-down adds
/// no second push then).
pub fn recently_knocked(actor: u32) -> bool {
    KNOCKED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|k| k.0 == actor && k.1.elapsed() < KNOCK_COOLDOWN)
}

/// The duplicate guard of the game's own blast knock-down, logged.
pub fn note_duplicate(e: &Engine, actor: u32) {
    note(format!(
        "{}: duplicate guard: the game's KnockExplosion skipped (knocked down here a moment ago)",
        describe(e, actor)
    ));
}

/// `NiAVObject`'s collision object, a collision object's `bhkRigidBody`, a node's children and
/// their count and items (as `AddExplosionImpulse` walks them), `bhkRigidBody`'s `hkpRigidBody`,
/// its linear and angular velocity (16-byte vectors) and their setters, the actor's 3D.
const NODE_COLLISION_OBJECT: u32 = 0x0043_b610;
const COLLISION_RIGID_BODY: u32 = 0x006f_a820;
const CHILD_COUNT: u32 = 0x0043_b480;
const CHILD_AT: u32 = 0x0043_b4a0;
const HAVOK_BODY: u32 = 0x0062_0b80;
const LINEAR_VELOCITY: u32 = 0x0056_0dc0;
const ANGULAR_VELOCITY: u32 = 0x0056_0e50;
const SET_LINEAR_VELOCITY: u32 = 0x0056_16d0;
const SET_ANGULAR_VELOCITY: u32 = 0x0056_1800;
const ACTOR_3D: u32 = 0x0043_fcd0;
/// Game units per Havok unit (a float), a node's world translation.
const HK_TO_GAME: u32 = 0x011b_1864;
const WORLD_TRANSLATION: u32 = 0x8c;
/// `PathingLocation(const NiPoint3&, TESObjectREFR*)`, its position, `ResolveToClosestNavmeshAndTriangle`,
/// `IsValidLocation(radius)`.
const PATHING_LOCATION_AT: u32 = 0x006d_ce10;
const PATHING_POSITION: u32 = 0x04;
const RESOLVE_TO_NAVMESH: u32 = 0x006d_dc00;
const IS_VALID_LOCATION: u32 = 0x006d_ed70;
/// `TESObjectREFR::SetLocationOnReference`, the actor's character controller,
/// `bhkCharacterController::SetPosition`, `bhkCharacterProxy::SetLinearVelocity`.
const SET_LOCATION: u32 = 0x0057_5830;
const CHAR_CONTROLLER: u32 = 0x0093_06d0;
const CONTROLLER_SET_POSITION: u32 = 0x0056_20e0;
const CONTROLLER_SET_VELOCITY: u32 = 0x0092_6980;
/// `bhkCharacterController::VelocityMod` and `fVelocityTime`; `LookupFormByID`.
const VELOCITY_MOD: u32 = 0x500;
const VELOCITY_TIME: u32 = 0x524;
const PUSH_DELTA: u32 = 0x580;
const PUSH_COUNT: u32 = 0x590;
/// The controller's linear velocity (a Havok vector, `00807080`), a node's name.
const CONTROLLER_VELOCITY: u32 = 0x0080_7080;
const NODE_NAME: u32 = 0x08;
/// The ground brace (stage 3): for this long after the knock-down, while the pelvis is more than
/// this over the lowest bone (units) and dropping faster than this (units/s), the arm bones are
/// steered this share a frame toward (m/s forward along the fall, m/s down).
const BRACE_GROUND_FOR: Duration =
    Duration::from_millis((ragdoll_core::flinch::BRACE_GROUND_SECONDS * 1000.0) as u64);
const BRACE_GROUND_HEIGHT: f32 = ragdoll_core::flinch::BRACE_GROUND_HEIGHT;
const BRACE_GROUND_DROP: f32 = ragdoll_core::flinch::BRACE_GROUND_DROP;
const BRACE_GROUND_SHARE: f32 = ragdoll_core::flinch::BRACE_GROUND_SHARE;
const BRACE_GROUND_SPEED: (f32, f32) = ragdoll_core::flinch::BRACE_GROUND_SPEED;
const LOOKUP_FORM: u32 = 0x0048_39c0;
/// `hkpMotion::getMass` and a rigid body's motion.
const GET_MASS: u32 = 0x00c8_6430;
const MOTION: u32 = 0xe0;
/// `HitData`'s health damage.
const HIT_HEALTH_DAMAGE: u32 = 0x14;
/// `MiddleHighProcess::UpdateKnockState`: the get-up waits there ([`hold_get_up`]).
pub const UPDATE_KNOCK_STATE: u32 = 0x0092_0150;
/// The most nodes walked for one body.
const MAX_NODES: usize = 256;

/// Every rigid body of the actor's 3D: its node and its `hkpRigidBody`.
fn bodies(e: &mut Engine, actor: u32) -> Vec<(u32, u32)> {
    let root = e.call(ACTOR_3D, &args![actor]).u32();
    let mut found = Vec::new();
    let mut stack = vec![root];
    let mut seen = 0;
    while let Some(node) = stack.pop() {
        if node == 0 || seen >= MAX_NODES {
            continue;
        }
        seen += 1;
        let collision = e.call(NODE_COLLISION_OBJECT, &args![node]).u32();
        if collision != 0 {
            let body = e.call(COLLISION_RIGID_BODY, &args![collision]).u32();
            if body != 0 {
                let havok = e.call(HAVOK_BODY, &args![body]).u32();
                if havok != 0 {
                    found.push((node, havok));
                }
            }
        }
        let children = e.vcall(node, 0xc, &[]).u32();
        if children != 0 {
            let count = e.call(CHILD_COUNT, &args![children]).u32().min(64);
            for i in 0..count {
                stack.push(e.call(CHILD_AT, &args![children, i]).u32());
            }
        }
    }
    found
}

/// Game units per Havok unit (the game's float at `011b1864`; 7 when it can't be read).
fn hk_scale(e: &Engine) -> f32 {
    let s = e.mem.f32(HK_TO_GAME);
    if s.is_finite() && (0.5..100.0).contains(&s) {
        s
    } else {
        7.0
    }
}

fn linear(e: &mut Engine, body: u32) -> Option<[f32; 3]> {
    let at = e.call(LINEAR_VELOCITY, &args![body]).u32();
    (at != 0).then(|| read3(e, at))
}

/// A knock-down's launch: given right after `KnockExplosion`, checked the frames after.
#[derive(Clone, Copy, Debug)]
struct Launch {
    actor: u32,
    id: u32,
    direction: [f32; 3],
    /// Game units a second.
    speed: f32,
    frames: u32,
    tries: u8,
}

static LAUNCHES: Mutex<Vec<Launch>> = Mutex::new(Vec::new());

/// The launch added to every bone of `actor`'s ragdoll: the same velocity, `speed` game units a
/// second along `direction`. Answers the bones given it.
fn give_launch(e: &mut Engine, actor: u32, direction: [f32; 3], speed: f32) -> usize {
    let scale = hk_scale(e);
    let add = direction.map(|x| x * speed / scale);
    let bones = bodies(e, actor);
    // The active ragdoll's body's motion, when it was reacting (`crate::active_ragdoll`): the
    // ragdoll goes on with it (nothing jumps at the switch), the launch on top.
    let going = crate::active_ragdoll::body_velocities(actor);
    let mut given = 0;
    for &(node, body) in &bones {
        if let Some(v) = linear(e, body) {
            let v = going
                .iter()
                .find(|g| g.0 == node)
                .map_or(v, |g| g.1.map(|x| x / scale));
            set_velocity(
                e,
                SET_LINEAR_VELOCITY,
                body,
                [v[0] + add[0], v[1] + add[1], v[2] + add[2]],
            );
            given += 1;
        }
    }
    given
}

/// Once a frame: each launch checked while the body is down (knock state 2, or 1 once the game
/// has it lying): when its bones kept less than [`LAUNCH_KEPT`] of it along the hit, it is given
/// again (at most [`LAUNCH_TRIES`]). The 535c53a build waited for knock state 2 a frame later,
/// which the game had already left for 1, and dropped every launch.
fn launches(e: &mut Engine) {
    let pending: Vec<Launch> =
        std::mem::take(&mut *LAUNCHES.lock().unwrap_or_else(|p| p.into_inner()));
    if pending.is_empty() {
        return;
    }
    let scale = hk_scale(e);
    let mut keep = Vec::new();
    for mut l in pending {
        if !live(e, l.actor, l.id) {
            continue;
        }
        let process = e.mem.u32(l.actor + PROCESS);
        if process == 0 || matches!(e.mem.u32(l.actor + LIFE_STATE), 1 | 2) {
            continue;
        }
        l.frames += 1;
        let state = e.mem.u8(process + KNOCK_STATE) as i8;
        if !matches!(state, 1 | 2) {
            note(format!(
                "{}: launch check ended: knock state {state}",
                describe(e, l.actor)
            ));
            continue;
        }
        let bones = bodies(e, l.actor);
        let along: Vec<f32> = bones
            .iter()
            .filter_map(|&(_, b)| linear(e, b))
            .map(|v| v[0] * l.direction[0] + v[1] * l.direction[1] + v[2] * l.direction[2])
            .collect();
        let kept = if along.is_empty() {
            0.0
        } else {
            along.iter().sum::<f32>() / along.len() as f32 * scale
        };
        if kept < l.speed * LAUNCH_KEPT && l.tries < LAUNCH_TRIES && !bones.is_empty() {
            l.tries += 1;
            let given = give_launch(e, l.actor, l.direction, l.speed);
            note(format!(
                "{}: duplicate guard: the launch was taken back ({kept:.0} of {:.0} units/s left, \
                 knock state {state}): given again to {given} bones ({}/{LAUNCH_TRIES})",
                describe(e, l.actor),
                l.speed,
                l.tries
            ));
            keep.push(l);
        } else {
            note(format!(
                "{}: the launch holds: {kept:.0} units/s along it (knock state {state})",
                describe(e, l.actor)
            ));
        }
    }
    LAUNCHES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .extend(keep);
}

/// A knocked-down actor waiting to get up.
#[derive(Clone, Copy, Debug)]
struct GetUp {
    actor: u32,
    id: u32,
    /// Where it stood when knocked down (a clear spot: the fallback).
    start: [f32; 3],
    since: Instant,
    /// Since it lay down (knock state 1 or 3) and the get-up began to be held.
    lying_since: Option<Instant>,
    still_frames: u32,
    held_said: bool,
    /// Placed at its spot: the game's get-up may run.
    allowed: bool,
    /// When and where it was set for the get-up, and the watch's next log time (seconds after).
    placed: Option<(Instant, [f32; 3])>,
    next_log: f32,
    /// The fall's direction (the launch's), and whether its arms braced on the ground.
    fall_dir: [f32; 3],
    braced: bool,
}

static GET_UPS: Mutex<Vec<GetUp>> = Mutex::new(Vec::new());

/// Whether `actor` is down or getting up (followed by the get-up, its watch included).
pub fn in_get_up(actor: u32) -> bool {
    GET_UPS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|g| g.actor == actor)
}

/// Whether the game's get-up of `actor` (its `UpdateKnockState`) waits this frame.
pub fn hold_get_up(e: &Engine, process: u32, actor: u32) -> bool {
    let state = e.mem.u8(process + KNOCK_STATE) as i8;
    if !matches!(state, 1 | 3) {
        return false;
    }
    GET_UPS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|g| g.actor == actor && !g.allowed)
}

/// Once a frame (the main thread): each knocked-down body held still and damped while it lies,
/// and its get-up let go once it is still and lying ([`may_get_up`]): the actor and its character
/// controller set where the body lies with no velocity ([`fit_spot`]), then watched
/// ([`watch_get_up`]).
fn get_ups(e: &mut Engine) {
    let list: Vec<GetUp> = GET_UPS.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if list.is_empty() {
        return;
    }
    let scale = hk_scale(e);
    let mut keep = Vec::new();
    for mut g in list {
        if !live(e, g.actor, g.id) {
            continue;
        }
        let process = e.mem.u32(g.actor + PROCESS);
        if process == 0
            || matches!(e.mem.u32(g.actor + LIFE_STATE), 1 | 2)
            || g.since.elapsed() > Duration::from_secs(30)
        {
            continue;
        }
        let state = e.mem.u8(process + KNOCK_STATE) as i8;
        if let Some((at, spot)) = g.placed {
            // The watch after the get-up (module documentation): kept for [`WATCH`].
            if watch_get_up(e, &mut g, process, state, at, spot, scale) {
                keep.push(g);
            }
            continue;
        }
        if state == 0 && g.since.elapsed() > Duration::from_millis(500) {
            continue; // up again
        }
        if matches!(state, 1 | 2) {
            // While it flies every bone is held to the bounds; while it lies only an arm
            // spinning past a person's flung arm is (a bone under its bound is never touched, so
            // the body still sleeps).
            hold_bones(e, g.actor, scale, g.since.elapsed() >= FLIGHT);
            if g.since.elapsed() < BRACE_GROUND_FOR {
                brace_ground(e, &mut g, scale);
            }
            // A hit on the lying body stores a knockback in its character controller, which the
            // controller only spends when it moves again: at the get-up, when it threw the actor.
            if let Some(stored) = clear_stored_push(e, g.actor) {
                note(format!(
                    "{}: duplicate guard: a stored controller push of {stored:.0} cleared while it lies",
                    describe(e, g.actor)
                ));
            }
        }
        if g.allowed || !matches!(state, 1 | 3) {
            keep.push(g);
            continue;
        }
        let lying_since = *g.lying_since.get_or_insert_with(Instant::now);
        // The body: its bones' positions (game units) and fastest speed.
        let bones = bodies(e, g.actor);
        let mut fastest = 0.0f32;
        let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut sum = [0.0f32; 3];
        for &(node, body) in &bones {
            let at = read3(e, node + WORLD_TRANSLATION);
            low = low.min(at[2]);
            high = high.max(at[2]);
            for i in 0..3 {
                sum[i] += at[i];
            }
            if let Some(v) = linear(e, body) {
                let s = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() * scale;
                fastest = fastest.max(if s.is_nan() { f32::INFINITY } else { s });
            }
        }
        let height = if bones.is_empty() { 0.0 } else { high - low };
        g.still_frames = if fastest < STILL_SPEED {
            g.still_frames + 1
        } else {
            0
        };
        let held = lying_since.elapsed();
        if !may_get_up(g.still_frames, height, held) {
            if !g.held_said {
                g.held_said = true;
                note(format!(
                    "{}: get-up held: {} (fastest bone {fastest:.0} units/s, body {height:.0} units \
                     tall)",
                    describe(e, g.actor),
                    if fastest >= STILL_SPEED {
                        "still moving"
                    } else {
                        "not lying on the ground"
                    }
                ));
            }
            keep.push(g);
            continue;
        }
        let centre = if bones.is_empty() {
            g.start
        } else {
            sum.map(|x| x / bones.len() as f32)
        };
        // Where the ragdoll ended up: its pelvis (else its bones' centre), on the ground under it.
        let pelvis = pelvis_of(e, &bones).unwrap_or(centre);
        let (ground, ground_why) = if bones.is_empty() {
            (g.start[2], "where it stood (no bones)")
        } else {
            ground_under(e, g.actor, pelvis, low)
        };
        let body_at = [pelvis[0], pelvis[1], ground];
        let fit = fit_spot(e, g.actor, body_at);
        let at = fit.at;
        // Nothing carried into the get-up: the bones still, the controller's stored push gone.
        for &(_, body) in &bones {
            set_velocity(e, SET_LINEAR_VELOCITY, body, [0.0; 3]);
            set_velocity(e, SET_ANGULAR_VELOCITY, body, [0.0; 3]);
        }
        let _ = clear_stored_push(e, g.actor);
        place(e, g.actor, at);
        g.allowed = true;
        g.placed = Some((Instant::now(), at));
        g.next_log = 0.0;
        note(format!(
            "{}: get-up allowed after {:.1} s ({}): the body's pelvis at ({:.0}, {:.0}, {:.0}), \
             ground {ground:.0} ({ground_why}); set at ({:.0}, {:.0}, {:.0}), {:.0} units from it ({}); \
             bones, controller velocity and stored push zeroed",
            describe(e, g.actor),
            held.as_secs_f32(),
            if held >= HOLD_MAX {
                "held too long"
            } else {
                "still and lying"
            },
            pelvis[0],
            pelvis[1],
            pelvis[2],
            at[0],
            at[1],
            at[2],
            fit.moved,
            fit.why
        ));
        keep.push(g);
    }
    *GET_UPS.lock().unwrap_or_else(|p| p.into_inner()) = keep;
}

/// The world position of the ragdoll's pelvis (the node named `Bip01 Pelvis`, `m_kName` at
/// `+0x08`), when it has one.
fn pelvis_of(e: &Engine, bones: &[(u32, u32)]) -> Option<[f32; 3]> {
    bones.iter().find_map(|&(node, _)| {
        let name = e.mem.u32(node + NODE_NAME);
        (name != 0 && e.mem.cstr(name).eq_ignore_ascii_case(b"Bip01 Pelvis"))
            .then(|| read3(e, node + WORLD_TRANSLATION))
    })
}

/// The ground under the body (`near` its pelvis, `low` its lowest bone's height): the walkable
/// floor there (the game's navmesh triangle under the pelvis, `ResolveToClosestNavmeshAndTriangle`,
/// when within [`GROUND_REACH`] across and between [`GROUND_BELOW`] under and [`BONE_RADIUS`]
/// over the lowest bone); else the lowest bone less [`BONE_RADIUS`] (a bone lies on the ground:
/// the down cast the ragdoll itself made). Answers the height and how it was found.
fn ground_under(e: &mut Engine, actor: u32, near: [f32; 3], low: f32) -> (f32, &'static str) {
    let from_bones = low - BONE_RADIUS;
    match navmesh_point(e, actor, [near[0], near[1], low]) {
        Some(at)
            if ((at[0] - near[0]).powi(2) + (at[1] - near[1]).powi(2)).sqrt() <= GROUND_REACH
                && at[2] >= low - GROUND_BELOW
                && at[2] <= low + BONE_RADIUS =>
        {
            (at[2], "the navmesh floor under the pelvis")
        }
        _ => (from_bones, "the lowest bone, less its radius"),
    }
}

/// The point `near` moved onto the closest navmesh triangle of the actor's cell, and whether a
/// capsule of `radius` stands clear there (`IsValidLocation`: line of sight `radius` away four
/// ways, so no wall or static within it).
fn navmesh_check(
    e: &mut Engine,
    actor: u32,
    near: [f32; 3],
    radius: f32,
) -> Option<([f32; 3], bool)> {
    e.with_stack(0x60, |e, frame| {
        let location = (frame.addr() + 15) & !15;
        let point = location + 0x40;
        for (i, x) in near.into_iter().enumerate() {
            e.mem.set_f32(point + 4 * i as u32, x);
        }
        e.call(PATHING_LOCATION_AT, &args![location, point, actor]);
        if !e.call(RESOLVE_TO_NAVMESH, &args![location]).bool() {
            return None;
        }
        let at = read3(e, location + PATHING_POSITION);
        if !at.iter().all(|x| x.is_finite()) {
            return None;
        }
        let clear = radius <= 0.0 || e.call(IS_VALID_LOCATION, &args![location, radius]).bool();
        Some((at, clear))
    })
}

pub(crate) fn navmesh_point(e: &mut Engine, actor: u32, near: [f32; 3]) -> Option<[f32; 3]> {
    navmesh_check(e, actor, near, 0.0).map(|(at, _)| at)
}

/// The capsule radius of an actor (game units): [`CAPSULE_RADIUS`] times its scale.
fn capsule(e: &mut Engine, actor: u32) -> f32 {
    let scale = e.call(GET_SCALE, &args![actor]).f64() as f32;
    CAPSULE_RADIUS * sane_scale(scale)
}

/// The other standing actors (high and middle-high processes, alive, not knocked down) whose
/// capsule a capsule of `radius` at `at` would overlap: (actor, distance across).
fn overlapping(e: &mut Engine, actor: u32, at: [f32; 3], radius: f32) -> Vec<(u32, f32)> {
    let mut found = Vec::new();
    for other in jip::commands::jip_fn_effect::high_actors(e) {
        if other == 0 || other == actor || matches!(e.mem.u32(other + LIFE_STATE), 1 | 2) {
            continue;
        }
        let process = e.mem.u32(other + PROCESS);
        if process != 0
            && e.mem.u8(process + PROCESS_LEVEL) <= MIDDLE_HIGH
            && e.mem.u8(process + KNOCK_STATE) as i8 == 2
        {
            continue; // a ragdoll, not a capsule
        }
        let p = read3(e, other + REFR_POSITION);
        let across = ((p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2)).sqrt();
        let need = radius + capsule(e, other);
        if across < need && (p[2] - at[2]).abs() < CAPSULE_HEIGHT {
            found.push((other, across));
        }
    }
    found
}

/// Where the get-up stands: the body's spot when its capsule fits there; else the nearest nudge
/// ([`NUDGES`], at most [`MAX_NUDGE`]) where it fits clear of walls and statics (the navmesh's
/// line of sight) and of other actors' capsules; else the body's spot anyway (never a far point).
#[derive(Clone, Copy, Debug)]
struct Fit {
    at: [f32; 3],
    moved: f32,
    why: &'static str,
}

fn fit_spot(e: &mut Engine, actor: u32, body: [f32; 3]) -> Fit {
    let radius = capsule(e, actor);
    let fits = |e: &mut Engine, p: [f32; 3]| -> Option<[f32; 3]> {
        if !overlapping(e, actor, p, radius).is_empty() {
            return None;
        }
        match navmesh_check(e, actor, p, radius) {
            Some((at, true))
                if ((at[0] - p[0]).powi(2) + (at[1] - p[1]).powi(2)).sqrt() <= NUDGE_SNAP
                    && (at[2] - p[2]).abs() <= GROUND_BELOW =>
            {
                Some([p[0], p[1], p[2].min(at[2].max(p[2] - GROUND_BELOW))])
            }
            // No navmesh here (some places have none): free of other actors is all that is known.
            None => Some(p),
            _ => None,
        }
    };
    if let Some(at) = fits(e, body) {
        return Fit {
            at,
            moved: 0.0,
            why: "the capsule fits at the body",
        };
    }
    for ring in NUDGES {
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            let p = [body[0] + ring * a.cos(), body[1] + ring * a.sin(), body[2]];
            if let Some(at) = fits(e, p) {
                return Fit {
                    at,
                    moved: ring,
                    why: "nudged to the nearest spot where the capsule fits",
                };
            }
        }
    }
    let crowd = overlapping(e, actor, body, radius);
    Fit {
        at: body,
        moved: 0.0,
        why: if crowd.is_empty() {
            "no clear spot within reach: left at the body (statics close)"
        } else {
            "no clear spot within reach: left at the body (actors close)"
        },
    }
}

/// The first [`WATCH`] after the get-up began (module documentation): the controller's stored
/// push and pushes zeroed for [`HOLD_PUSH`], its vertical speed held within
/// [`MAX_RISE`] up, the bones' upward speed likewise, and a log line every [`WATCH_STEP`]
/// (position, controller velocity, other actors overlapping, how far it was moved off its spot).
/// False when the watch is over.
fn watch_get_up(
    e: &mut Engine,
    g: &mut GetUp,
    process: u32,
    state: i8,
    since: Instant,
    spot: [f32; 3],
    scale: f32,
) -> bool {
    let t = since.elapsed().as_secs_f32();
    if t > WATCH.as_secs_f32() {
        note(format!(
            "{}: get-up watch over (knock state {state})",
            describe(e, g.actor)
        ));
        return false;
    }
    let _ = process;
    let controller = e.call(CHAR_CONTROLLER, &args![g.actor]).u32();
    let mut cleared = None;
    let mut clamped = None;
    let mut velocity = [0.0f32; 3];
    if controller != 0 {
        if t < HOLD_PUSH.as_secs_f32() {
            cleared = clear_stored_push(e, g.actor);
        }
        let at = e.call(CONTROLLER_VELOCITY, &args![controller]).u32();
        if at != 0 {
            velocity = read3(e, at).map(|x| x * scale);
            if velocity[2] > MAX_RISE {
                let held = [velocity[0], velocity[1], MAX_RISE];
                set_controller_velocity(e, controller, held);
                clamped = Some(velocity[2]);
            }
        }
    }
    // The ragdoll's bones while the get-up blends: none may fly up.
    let most_up = MAX_RISE / scale;
    let mut bones_held = 0;
    for (_, body) in bodies(e, g.actor) {
        if let Some(v) = linear(e, body) {
            if v[2] > most_up {
                set_velocity(e, SET_LINEAR_VELOCITY, body, [v[0], v[1], most_up]);
                bones_held += 1;
            }
        }
    }
    let p = read3(e, g.actor + REFR_POSITION);
    let off = [p[0] - spot[0], p[1] - spot[1], p[2] - spot[2]];
    let across = (off[0] * off[0] + off[1] * off[1]).sqrt();
    let radius = capsule(e, g.actor);
    let crowd = overlapping(e, g.actor, p, radius);
    let due = t >= g.next_log;
    let alarm = clamped.is_some() || bones_held > 0 || cleared.is_some() || !crowd.is_empty();
    if due || (alarm && t >= g.next_log - WATCH_STEP.as_secs_f32() / 2.0) {
        g.next_log = t + WATCH_STEP.as_secs_f32();
        note(format!(
            "{}: get-up +{t:.2} s (knock state {state}): at ({:.0}, {:.0}, {:.0}), moved off its spot \
             {across:.0} across and {:.0} up (depenetration); controller velocity ({:.0}, {:.0}, \
             {:.0}) units/s{}{}{}{}",
            describe(e, g.actor),
            p[0],
            p[1],
            p[2],
            off[2],
            velocity[0],
            velocity[1],
            velocity[2],
            clamped.map_or(String::new(), |v| format!(
                "; clamp: rising {v:.0} held to {MAX_RISE}"
            )),
            if bones_held > 0 {
                format!("; clamp: {bones_held} bones rising held")
            } else {
                String::new()
            },
            cleared.map_or(String::new(), |c| format!("; stored push {c:.0} cleared")),
            if crowd.is_empty() {
                String::new()
            } else {
                format!(
                    "; overlapping {}",
                    crowd
                        .iter()
                        .map(|(a, d)| format!("{:08X} at {d:.0}", e.mem.u32(a + FORM_ID)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        ));
    }
    true
}

/// The controller's velocity set to `v` (game units a second; `bhkCharacterProxy::SetLinearVelocity`).
fn set_controller_velocity(e: &mut Engine, controller: u32, v: [f32; 3]) {
    e.with_stack(16, |e, frame| {
        let at = frame.addr();
        for (i, x) in v.into_iter().enumerate() {
            e.mem.set_f32(at + 4 * i as u32, x);
        }
        e.call(CONTROLLER_SET_VELOCITY, &args![controller, at]);
    });
}

/// The actor's reference and character controller set at `at`, the controller's velocity zero.
pub(crate) fn place(e: &mut Engine, actor: u32, at: [f32; 3]) {
    e.with_stack(32, |e, frame| {
        let point = frame.addr();
        let zero = frame.addr() + 16;
        for (i, x) in at.into_iter().enumerate() {
            e.mem.set_f32(point + 4 * i as u32, x);
            e.mem.set_f32(zero + 4 * i as u32, 0.0);
        }
        e.call(SET_LOCATION, &args![actor, point]);
        let controller = e.call(CHAR_CONTROLLER, &args![actor]).u32();
        if controller != 0 {
            e.call(CONTROLLER_SET_POSITION, &args![controller, point]);
            e.call(CONTROLLER_SET_VELOCITY, &args![controller, zero]);
        }
    });
}

fn read3(e: &Engine, at: u32) -> [f32; 3] {
    [0, 4, 8].map(|o| e.mem.f32(at + o))
}

/// Sets a body's velocity (`setter`) to `v` through an aligned stack vector.
fn set_velocity(e: &mut Engine, setter: u32, body: u32, v: [f32; 3]) {
    e.with_stack(32, |e, frame| {
        let at = (frame.addr() + 15) & !15;
        for (i, x) in v.into_iter().enumerate() {
            e.mem.set_f32(at + 4 * i as u32, x);
        }
        e.mem.set_f32(at + 12, 0.0);
        e.call(setter, &args![body, at]);
    });
}

/// A knocked-down actor's bones while it flies: only a bone faster than [`MAX_BONE_SPEED`] or
/// spinning faster than [`MAX_BONE_SPIN`] is touched (held to it); bones under the bounds are
/// never set, so Havok can let the body come to rest and sleep. (274ad9c and 535c53a also damped
/// every slow bone each frame, which woke the whole ragdoll every frame: it never slept and kept
/// trembling.)
fn hold_bones(e: &mut Engine, actor: u32, scale: f32, arms_only: bool) {
    let mut clamped = 0;
    let mut fastest = 0.0f32;
    let most = MAX_BONE_SPEED / scale;
    let arm_spin = ragdoll_core::tuning::get().arm_ragdoll_spin;
    for (node, body) in bodies(e, actor) {
        let arm = is_arm_node(e, node);
        if arms_only && !arm {
            continue;
        }
        let at = e.call(LINEAR_VELOCITY, &args![body]).u32();
        let spin_at = e.call(ANGULAR_VELOCITY, &args![body]).u32();
        if at == 0 || spin_at == 0 {
            continue;
        }
        let v = read3(e, at);
        let w = read3(e, spin_at);
        let (nv, a) = if arms_only {
            (v, false)
        } else {
            clamp_speed(v, most)
        };
        // The arms spin no faster than a person's flung arm (Patrick, 2026-11-15: they flailed).
        let spin = if arm {
            arm_spin.min(MAX_BONE_SPIN)
        } else {
            MAX_BONE_SPIN
        };
        let (nw, b) = clamp_speed(w, spin);
        if a {
            set_velocity(e, SET_LINEAR_VELOCITY, body, nv);
            let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            fastest = fastest.max(if speed.is_nan() {
                f32::INFINITY
            } else {
                speed * scale
            });
        }
        if b {
            set_velocity(e, SET_ANGULAR_VELOCITY, body, nw);
        }
        clamped += usize::from(a || b);
    }
    if clamped > 0 {
        note(format!(
            "{}: clamp: {clamped} bones held (fastest {fastest:.0} units/s, at most {MAX_BONE_SPEED} \
             and spin {MAX_BONE_SPIN}, arms {arm_spin}){}",
            describe(e, actor),
            if arms_only { " (lying: arms only)" } else { "" }
        ));
    }
}

/// Whether a ragdoll bone's node is an arm's (a clavicle, an upper arm, a forearm, a hand).
fn is_arm_node(e: &Engine, node: u32) -> bool {
    let name = e.mem.u32(node + NODE_NAME);
    if name == 0 {
        return false;
    }
    let text = e.mem.cstr(name);
    BONES_ARMS
        .iter()
        .any(|n| text.eq_ignore_ascii_case(n.as_bytes()))
}

const BONES_ARMS: [&str; 8] = [
    "Bip01 L Clavicle",
    "Bip01 R Clavicle",
    "Bip01 L UpperArm",
    "Bip01 R UpperArm",
    "Bip01 L Forearm",
    "Bip01 R Forearm",
    "Bip01 L Hand",
    "Bip01 R Hand",
];

/// Stage 3's brace on the ground: while the falling body is still well off the ground and
/// dropping, its hands and forearms (the arms it has) are steered forward along the fall and down
/// ([`BRACE_GROUND_SPEED`]), a share a frame, so the arms go out to take the landing.
fn brace_ground(e: &mut Engine, g: &mut GetUp, scale: f32) {
    let limbs = crate::balance::limbs_of(g.actor);
    let bones = bodies(e, g.actor);
    let named = |e: &Engine, node: u32, want: &[u8]| {
        let name = e.mem.u32(node + NODE_NAME);
        name != 0 && e.mem.cstr(name).eq_ignore_ascii_case(want)
    };
    let mut low = f32::INFINITY;
    let mut pelvis = None;
    for &(node, body) in &bones {
        let at = read3(e, node + WORLD_TRANSLATION);
        low = low.min(at[2]);
        if named(e, node, b"Bip01 Pelvis") {
            pelvis = Some((at, body));
        }
    }
    let Some((at, body)) = pelvis else {
        return;
    };
    let dropping = linear(e, body).is_some_and(|v| v[2] * scale < -BRACE_GROUND_DROP);
    if at[2] - low < BRACE_GROUND_HEIGHT || !dropping {
        return;
    }
    let dir = [g.fall_dir[0], g.fall_dir[1], 0.0];
    let flat = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    let dir = if flat > 1e-3 {
        [dir[0] / flat, dir[1] / flat, 0.0]
    } else {
        [0.0; 3]
    };
    let want = [
        dir[0] * BRACE_GROUND_SPEED.0 * UNITS_PER_METRE / scale,
        dir[1] * BRACE_GROUND_SPEED.0 * UNITS_PER_METRE / scale,
        -BRACE_GROUND_SPEED.1 * UNITS_PER_METRE / scale,
    ];
    let mut moved = 0;
    for &(node, body) in &bones {
        let arm = [
            (b"Bip01 L Hand".as_slice(), limbs.left_arm),
            (b"Bip01 L Forearm".as_slice(), limbs.left_arm),
            (b"Bip01 R Hand".as_slice(), limbs.right_arm),
            (b"Bip01 R Forearm".as_slice(), limbs.right_arm),
        ]
        .iter()
        .any(|(name, has)| *has && named(e, node, name));
        if !arm {
            continue;
        }
        if let Some(v) = linear(e, body) {
            let nv = [0, 1, 2].map(|i| v[i] + (want[i] - v[i]) * BRACE_GROUND_SHARE);
            let (nv, _) = clamp_speed(nv, MAX_BONE_SPEED / scale);
            set_velocity(e, SET_LINEAR_VELOCITY, body, nv);
            moved += 1;
        }
    }
    if moved > 0 && !g.braced {
        g.braced = true;
        note(format!(
            "{}: braces for the ground with its arms ({moved} arm bones)",
            describe(e, g.actor)
        ));
    }
}

/// The knockback the actor's character controller holds for its next move (`VelocityMod`,
/// `+0x500`, and `fVelocityTime`, `+0x524`, as `SetVelocityModifier` stores them) and the pushes
/// other characters' controllers gave it (`PushDelta` `+0x580`, `iPushCount` `+0x590`:
/// `characterInteractionCallback`, `00c6f500`, every frame one walks into the knocked-down actor's
/// standing capsule, with a 0.01 s time, a hundredfold velocity) cleared; its size (game units a
/// second, roughly) when there was one.
fn clear_stored_push(e: &mut Engine, actor: u32) -> Option<f32> {
    let controller = e.call(CHAR_CONTROLLER, &args![actor]).u32();
    if controller == 0 {
        return None;
    }
    let v = read3(e, controller + VELOCITY_MOD);
    let time = e.mem.f32(controller + VELOCITY_TIME);
    let size = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let stored = time != 0.0 || size != 0.0;
    for i in 0..4 {
        e.mem.set_f32(controller + VELOCITY_MOD + 4 * i, 0.0);
        e.mem.set_f32(controller + PUSH_DELTA + 4 * i, 0.0);
    }
    e.mem.set_f32(controller + VELOCITY_TIME, 0.0);
    e.mem.set_u32(controller + PUSH_COUNT, 0);
    (stored && size.is_finite() && size > 1e-3).then(|| size * hk_scale(e) * time.max(0.0))
}

/// Whether `actor` (read with its form id `id` when it was first seen) is still that actor: the
/// game's form map gives the same pointer for the id. A load, a new game, the main menu or a
/// cell's unload frees references; an entry kept across one is dropped, never read.
fn live(e: &mut Engine, actor: u32, id: u32) -> bool {
    actor != 0 && id != 0 && e.call(LOOKUP_FORM, &args![id]).u32() == actor
}

/// Every knock-down's bookkeeping dropped (a load, a new game, the main menu:
/// `crate::test_range`'s end seams).
pub fn clear() {
    DOWN.lock().unwrap_or_else(|p| p.into_inner()).clear();
    LAUNCHES.lock().unwrap_or_else(|p| p.into_inner()).clear();
    GET_UPS.lock().unwrap_or_else(|p| p.into_inner()).clear();
    KNOCKED.lock().unwrap_or_else(|p| p.into_inner()).clear();
    STACKS.lock().unwrap_or_else(|p| p.into_inner()).clear();
    UP.lock().unwrap_or_else(|p| p.into_inner()).clear();
    PENDING.lock().unwrap_or_else(|p| p.into_inner()).clear();
    ATTEMPTS.lock().unwrap_or_else(|p| p.into_inner()).clear();
}

/// A knocked-down actor's landing after a drop of `drop` units (module documentation).
fn land(e: &mut Engine, actor: u32, aggressor: u32, drop: f32) {
    let damage = fall_damage(
        drop,
        setting_f(e, FALL_MIN),
        setting_f(e, FALL_EXPONENT),
        setting_f(e, FALL_MULT),
    );
    if damage <= 0.0 {
        return;
    }
    // The game's damage takes an attacker actor: a trap's or a mine's blast has none (its
    // aggressor is the terminal's or the mine's reference).
    let aggressor = if jip::commands::is_actor(e, aggressor) {
        aggressor
    } else {
        0
    };
    e.vcall(actor, SLOT_DAMAGE, &args![damage, 0.0f32, aggressor]);
    let limb = damage * setting_f(e, LIMB_MULT);
    let chance_at = e.call(SETTING_INT, &args![LIMB_CHANCE]).u32();
    let chance = if chance_at == 0 {
        0
    } else {
        e.mem.u32(chance_at)
    };
    for leg in LEGS {
        let roll = e.call(RANDOM, &[]).u32() % 100;
        if roll < chance {
            e.vcall(actor, SLOT_LIMB, &args![leg, -limb, aggressor]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The get-up waits for a still, lying body, never longer than its limit.
    #[test]
    fn the_get_up_waits_for_a_still_lying_body() {
        let soon = Duration::from_millis(500);
        assert!(!may_get_up(0, 20.0, soon), "moving");
        assert!(
            !may_get_up(STILL_FRAMES, 150.0, soon),
            "upright against a wall"
        );
        assert!(may_get_up(STILL_FRAMES, 30.0, soon));
        assert!(may_get_up(0, 150.0, HOLD_MAX), "held too long");
    }

    /// A bone's speed is held to its bound along its own direction; a NaN stops it.
    #[test]
    fn bone_speeds_are_held() {
        let (v, held) = clamp_speed([30.0, 40.0, 0.0], 10.0);
        assert!(held);
        assert!((v[0] - 6.0).abs() < 1e-4 && (v[1] - 8.0).abs() < 1e-4);
        assert_eq!(clamp_speed([1.0, 2.0, 2.0], 10.0), ([1.0, 2.0, 2.0], false));
        assert_eq!(clamp_speed([f32::NAN, 0.0, 0.0], 10.0), ([0.0; 3], true));
    }
}
