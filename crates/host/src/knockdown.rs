//! Knockdowns of living actors (Patrick, 2026-11-14: Skyrim-style), on the ragdoll hit force
//! ([`crate::hit_force`]). Part of the weighty combat row; `[nv-rs Gunplay] bKnockdowns`,
//! `iKnockdownThreshold` (both on the Settings menu's Gameplay page) and `bKnockdownCompanions`.
//! **Not 1:1 with FalloutNV.exe.** Never the player.
//!
//! **When.** Over `Actor::HitMe` (`0089a760`, every hit: projectiles, swings; on the main thread,
//! the task queue's hits when they come back), after the game applied the hit: for a target that
//! is alive, not the player, not a companion (unless `bKnockdownCompanions=1`), not already
//! knocked down (its process's knock state `+0x13c`), the hit's momentum is worked out as the hit
//! force works it out ([`crate::hit_force::momentum`]: damage less the damage threshold,
//! projectiles, weight, reach, Strength, skill, power attack). Blasts are left to the game, which
//! knocks actors down from them itself (scaled by `iHitForce` already).
//!
//! **How much it takes** ([`resistance`]): by body, in the same damage points, times the actor's
//! scale squared and `iKnockdownThreshold` percent: a human [`HUMAN`], a super mutant or nightkin
//! [`SUPER_MUTANT`], and by creature type (`0059f3a0`): animals, mutated animals and insects,
//! abominations (deathclaws), super mutants, feral ghouls, robots and giants ([`CREATURE`]).
//! - At or past the resistance: a **knockdown**. The game's own blast knock-down
//!   (`MiddleHighProcess::KnockExplosion`, `0091fee0`) on the actor: it leaves its furniture, goes
//!   into its ragdoll (knock state 2) and gets the game's push from a point [`ORIGIN_BACK`] units
//!   behind the impact along the hit's direction (the hit data's impact point `+0x38` and
//!   direction `+0x44`; for a swing without them, from the attacker through the chest; for a
//!   blast, from its centre). The push (`TESHavokUtilities::AddExplosionImpulse`, `0062b660`) adds
//!   to every bone's velocity its own direction away from that point times the strength times the
//!   bone's layer multiplier, with no falloff and no division by mass: so the strength is a
//!   speed, on the scale of the game's `PushActorAway` (`GetKnockbackSpeed`: tens), and the point
//!   is far enough back that all bones go the same way (a near one pulls the limbs apart, which
//!   the joints fight: the shaking). Strength: the momentum times [`FORCE_PER_POINT`] and
//!   `iHitForce` percent, between [`MIN_FORCE`] and [`MAX_FORCE`]. The game gets it up again
//!   (`UpdateKnockState`: the get-up idle) when it can.
//! - **Once:** an actor knocked down in the last [`KNOCK_COOLDOWN`] or got up in the last
//!   [`UP_COOLDOWN`] is not knocked again (no knock and get-up flipping), the game's own blast
//!   knock-down ([`crate::hit_force::KNOCK_EXPLOSION`]) adds no second push to an actor knocked
//!   here a moment ago ([`recently_knocked`]), and JIP's stagger push left on the character
//!   controller is cleared before the ragdoll takes over. While it is down ([`frame`]), each bone's
//!   speed is held to [`MAX_BONE_SPEED`] and its spin to [`MAX_BONE_SPIN`] for [`FLIGHT`], and
//!   then, once a bone moves slower than [`SETTLE_SPEED`], its sideways motion and spin are damped
//!   ([`SETTLE_DAMPING`] a second) so the settling body lies still instead of jittering. Every
//!   clamp and guard is logged.
//! - At or past [`STAGGER_SHARE`] of it: a **stagger**, JIP's push without a ragdoll (#565,
//!   `jip::push_living_actor`): a shove of [`STAGGER_PER_POINT`] per point along the hit.
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

use crate::hit_force::{momentum, shot_of, Shot};
use engine::prelude::*;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The tuning constants (module documentation).
pub const HUMAN: f32 = 45.0;
pub const SUPER_MUTANT: f32 = 150.0;
/// By creature type: animal, mutated animal, mutated insect, abomination, super mutant, feral
/// ghoul, robot, giant.
pub const CREATURE: [f32; 8] = [30.0, 60.0, 50.0, 200.0, 150.0, 35.0, 120.0, 400.0];
pub const STAGGER_SHARE: f32 = 0.4;
pub const FORCE_PER_POINT: f32 = 0.35;
pub const MIN_FORCE: f32 = 8.0;
pub const MAX_FORCE: f32 = 40.0;
pub const STAGGER_PER_POINT: f32 = 2.0;
pub const MAX_STAGGER: f32 = 200.0;
pub const ORIGIN_BACK: f32 = 400.0;
/// No second knock-down this soon after one, nor this soon after getting up.
pub const KNOCK_COOLDOWN: Duration = Duration::from_millis(2500);
pub const UP_COOLDOWN: Duration = Duration::from_millis(1500);
/// A bone's speed (Havok units a second, about metres) and spin (radians a second) while it flies,
/// how long that is held, and the settling: below this speed, sideways motion and spin lose this
/// share a second.
pub const MAX_BONE_SPEED: f32 = 12.0;
pub const MAX_BONE_SPIN: f32 = 15.0;
pub const FLIGHT: Duration = Duration::from_millis(1500);
pub const SETTLE_SPEED: f32 = 1.5;
pub const SETTLE_DAMPING: f32 = 6.0;
pub const FOLLOW_FOR: Duration = Duration::from_secs(12);

/// What kind of body takes the hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Body {
    Human,
    SuperMutant,
    /// A creature of the game's type (0 to 7; anything else counts as an animal).
    Creature(i8),
}

/// The momentum it takes to knock `body` down: by body, times `scale` squared and `threshold`
/// percent.
pub fn resistance(body: Body, scale: f32, threshold: i32) -> f32 {
    let base = match body {
        Body::Human => HUMAN,
        Body::SuperMutant => SUPER_MUTANT,
        Body::Creature(kind) => CREATURE
            .get(usize::try_from(kind).unwrap_or(0))
            .copied()
            .unwrap_or(CREATURE[0]),
    };
    let scale = if scale.is_finite() {
        scale.clamp(0.25, 4.0)
    } else {
        1.0
    };
    base * scale * scale * threshold.max(0) as f32 / 100.0
}

/// What a hit of `momentum` does to a body of `resistance`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    None,
    Stagger,
    Knockdown,
}

pub fn reaction(momentum: f32, resistance: f32) -> Reaction {
    if momentum.is_nan() || momentum <= 0.0 {
        Reaction::None
    } else if momentum >= resistance {
        Reaction::Knockdown
    } else if momentum >= resistance * STAGGER_SHARE {
        Reaction::Stagger
    } else {
        Reaction::None
    }
}

/// The game's fall damage for a drop of `drop` units: `(drop - min)^exponent * mult` past `min`.
pub fn fall_damage(drop: f32, min: f32, exponent: f32, mult: f32) -> f32 {
    if drop.is_nan() || drop <= min || !exponent.is_finite() || !mult.is_finite() {
        return 0.0;
    }
    ((drop - min).powf(exponent) * mult).max(0.0)
}

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
static DOWN: Mutex<Vec<(u32, u32, f32, Instant)>> = Mutex::new(Vec::new());

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
    let stats = crate::gunplay::effective_stats(e, aggressor, e.mem.u32(hit + HIT_WEAPON));
    let shot = shot_of(e, hit, stats.as_ref());
    // A blast's damage already has the threshold taken; it pushes away from its centre.
    let (threshold, (point, direction)) = match shot {
        Shot::Explosion { .. } => (0.0, blast(e, hit, target)),
        _ => (
            actor_value(e, target, AV_DAMAGE_THRESHOLD),
            impact(e, hit, target, aggressor),
        ),
    };
    let m = momentum(shot, threshold);
    let scale = e.call(GET_SCALE, &args![target]).f64() as f32;
    let body = body_of(e, target);
    let resist = resistance(body, scale, t.knockdown_threshold);
    let decision = reaction(m, resist);
    note(format!(
        "{who}: {} momentum {m:.1} (DT {threshold:.0}) vs resistance {resist:.1} ({body:?}, scale \
         {scale:.2}, threshold {}%) -> {decision:?}",
        kind_of(shot),
        t.knockdown_threshold
    ));
    match decision {
        Reaction::Knockdown => knock_down(e, target, aggressor, point, direction, m),
        Reaction::Stagger => {
            let force = (m * STAGGER_PER_POINT).min(MAX_STAGGER);
            let angle = direction[0].atan2(direction[1]).to_degrees();
            jip::push_living_actor(e, target, force, angle, false);
        }
        Reaction::None => {}
    }
}

fn kind_of(shot: Shot) -> &'static str {
    match shot {
        Shot::Gun { .. } => "gun",
        Shot::Melee { .. } => "melee",
        Shot::Explosion { .. } => "blast",
        Shot::Bullet { .. } => "bullet",
    }
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

/// The launch's strength for a momentum `m`: times [`FORCE_PER_POINT`] and `hit_force` percent,
/// between [`MIN_FORCE`] (a small push, so the body falls) and [`MAX_FORCE`].
pub fn launch_force(m: f32, hit_force: i32) -> f32 {
    let f = m * FORCE_PER_POINT * hit_force.max(0) as f32 / 100.0;
    if f.is_nan() {
        MIN_FORCE
    } else {
        f.clamp(MIN_FORCE, MAX_FORCE)
    }
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

/// A settling bone's velocity after `dt` seconds of damping: sideways (x, y) and spin lose
/// [`SETTLE_DAMPING`] a second, its fall (z) is left to gravity.
pub fn settle(v: [f32; 3], dt: f32, keep_z: bool) -> [f32; 3] {
    let k = (-SETTLE_DAMPING * dt.clamp(0.0, 0.1)).exp();
    [v[0] * k, v[1] * k, if keep_z { v[2] } else { v[2] * k }]
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
    m: f32,
) {
    use engine::units::fallout_ai::middlehighprocess::middle_high_process_knock_explosion;
    let process = e.mem.u32(actor + PROCESS);
    if process == 0 || actor == e.global::<u32>(PLAYER) {
        return;
    }
    if e.mem.u8(process + PROCESS_LEVEL) > MIDDLE_HIGH {
        return; // KnockExplosion is MiddleHighProcess's: a lower process has none
    }
    let t = crate::gunplay::tuning();
    let strength = launch_force(m, t.hit_force);
    let origin = [0, 1, 2].map(|i| point[i] - direction[i] * ORIGIN_BACK);
    let start = position(e, actor).map_or(origin[2], |p| p[2]);
    let who = describe(e, actor);
    if let Some(why) = cooling_down(actor) {
        note(format!("{who}: duplicate guard: {why}, not knocked again"));
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
        return;
    }
    let ragdoll = e.vcall(actor, 0x38c, &args![]).bool();
    let wanted = m * FORCE_PER_POINT * t.hit_force.max(0) as f32 / 100.0;
    if wanted > MAX_FORCE || wanted < MIN_FORCE {
        note(format!(
            "{who}: clamp: launch strength {wanted:.1} held to {strength:.1} ({MIN_FORCE}-{MAX_FORCE})"
        ));
    }
    // JIP's stagger push left on the character controller would keep moving it under the ragdoll.
    jip::push_living_actor(e, actor, 0.0, 0.0, false);
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
    down.push((actor, aggressor, start, Instant::now()));
}

/// Once a frame (from the player's look hook): each followed actor's highest point, and when it
/// is down, getting up or followed long enough, its fall's damage (module documentation).
pub fn frame(e: &mut Engine) {
    let followed: Vec<(u32, u32, f32, Instant)> =
        DOWN.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let dt = {
        let mut last = LAST_FRAME.lock().unwrap_or_else(|p| p.into_inner());
        let dt = last.map_or(0.0, |t| t.elapsed().as_secs_f32());
        *last = Some(Instant::now());
        dt
    };
    if followed.is_empty() {
        return;
    }
    let mut keep = Vec::new();
    for (actor, aggressor, top, since) in followed {
        let process = e.mem.u32(actor + PROCESS);
        let Some(at) = position(e, actor) else {
            continue;
        };
        if process == 0 || matches!(e.mem.u32(actor + LIFE_STATE), 1 | 2) {
            continue;
        }
        let top = top.max(at[2]);
        let state = e.mem.u8(process + KNOCK_STATE) as i8;
        if state == 2 {
            hold_bones(e, actor, since.elapsed() < FLIGHT, dt);
        }
        let settled = matches!(state, 0 | 1 | 3 | 5 | 6) || since.elapsed() > FOLLOW_FOR;
        if settled && since.elapsed() > Duration::from_millis(400) {
            land(e, actor, aggressor, top - at[2]);
            UP.lock()
                .unwrap_or_else(|p| p.into_inner())
                .push((actor, Instant::now()));
        } else {
            keep.push((actor, aggressor, top, since));
        }
    }
    *DOWN.lock().unwrap_or_else(|p| p.into_inner()) = keep;
}

/// The actors knocked down here and when, and those that got up again and when.
static KNOCKED: Mutex<Vec<(u32, Instant)>> = Mutex::new(Vec::new());
static UP: Mutex<Vec<(u32, Instant)>> = Mutex::new(Vec::new());
static LAST_FRAME: Mutex<Option<Instant>> = Mutex::new(None);

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
/// The most nodes walked for one body.
const MAX_NODES: usize = 256;

/// Every `hkpRigidBody` of the actor's 3D.
fn bodies(e: &mut Engine, actor: u32) -> Vec<u32> {
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
                    found.push(havok);
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

/// A knocked-down actor's bones: while it flies, held to [`MAX_BONE_SPEED`] and
/// [`MAX_BONE_SPIN`]; afterwards, a slow bone's sideways motion and spin damped ([`settle`]).
fn hold_bones(e: &mut Engine, actor: u32, flying: bool, dt: f32) {
    let mut clamped = 0;
    let mut fastest = 0.0f32;
    for body in bodies(e, actor) {
        let at = e.call(LINEAR_VELOCITY, &args![body]).u32();
        let spin_at = e.call(ANGULAR_VELOCITY, &args![body]).u32();
        if at == 0 || spin_at == 0 {
            continue;
        }
        let v = read3(e, at);
        let w = read3(e, spin_at);
        let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if flying {
            let (nv, a) = clamp_speed(v, MAX_BONE_SPEED);
            let (nw, b) = clamp_speed(w, MAX_BONE_SPIN);
            if a {
                set_velocity(e, SET_LINEAR_VELOCITY, body, nv);
                fastest = fastest.max(if speed.is_nan() { f32::INFINITY } else { speed });
            }
            if b {
                set_velocity(e, SET_ANGULAR_VELOCITY, body, nw);
            }
            clamped += usize::from(a || b);
        } else if speed < SETTLE_SPEED && dt > 0.0 {
            set_velocity(e, SET_LINEAR_VELOCITY, body, settle(v, dt, true));
            set_velocity(e, SET_ANGULAR_VELOCITY, body, settle(w, dt, false));
        }
    }
    if clamped > 0 {
        note(format!(
            "{}: clamp: {clamped} bones held (fastest {fastest:.1}, at most {MAX_BONE_SPEED} and \
             spin {MAX_BONE_SPIN})",
            describe(e, actor)
        ));
    }
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

    /// A deathclaw or a super mutant takes far more than a human, a giant more again; a bigger
    /// one more; the threshold scales all.
    #[test]
    fn bodies_resist_by_kind_and_size() {
        let human = resistance(Body::Human, 1.0, 100);
        assert_eq!(human, HUMAN);
        assert!(resistance(Body::SuperMutant, 1.0, 100) > 3.0 * human);
        assert!(
            resistance(Body::Creature(3), 1.0, 100) > 4.0 * human,
            "deathclaws"
        );
        assert!(
            resistance(Body::Creature(6), 1.0, 100) > 2.0 * human,
            "robots"
        );
        assert!(resistance(Body::Creature(7), 1.0, 100) > resistance(Body::Creature(3), 1.0, 100));
        assert!(resistance(Body::Creature(0), 1.5, 100) > resistance(Body::Creature(0), 1.0, 100));
        assert_eq!(resistance(Body::Human, 1.0, 200), 2.0 * HUMAN);
        assert_eq!(resistance(Body::Creature(-1), 1.0, 100), CREATURE[0]);
    }

    /// A hit at the resistance knocks down, a moderate one staggers, a light one does nothing.
    #[test]
    fn a_hit_knocks_down_staggers_or_not() {
        assert_eq!(reaction(50.0, 45.0), Reaction::Knockdown);
        assert_eq!(reaction(20.0, 45.0), Reaction::Stagger);
        assert_eq!(reaction(10.0, 45.0), Reaction::None);
        assert_eq!(reaction(f32::NAN, 45.0), Reaction::None);
        // A 50-damage rifle round knocks an unarmoured human down, not a deathclaw.
        let rifle = crate::hit_force::momentum(
            crate::hit_force::Shot::Gun {
                damage: 50.0,
                projectiles: 1,
            },
            0.0,
        );
        assert_eq!(
            reaction(rifle, resistance(Body::Human, 1.0, 100)),
            Reaction::Knockdown
        );
        assert_ne!(
            reaction(rifle, resistance(Body::Creature(3), 1.0, 100)),
            Reaction::Knockdown
        );
    }

    /// The game's fall formula: nothing within the minimum, more the further past it.
    #[test]
    fn falls_hurt_past_the_minimum() {
        assert_eq!(fall_damage(300.0, 400.0, 1.45, 0.1), 0.0);
        let low = fall_damage(600.0, 400.0, 1.45, 0.1);
        let high = fall_damage(1200.0, 400.0, 1.45, 0.1);
        assert!(low > 0.0 && high > 3.0 * low, "{low} {high}");
        assert_eq!(fall_damage(f32::NAN, 400.0, 1.45, 0.1), 0.0);
    }

    /// The launch grows with the momentum and Hit Force, within its bounds.
    #[test]
    fn the_launch_is_bounded() {
        assert!((launch_force(50.0, 100) - 17.5).abs() < 1e-4);
        assert!((launch_force(50.0, 200) - 35.0).abs() < 1e-4);
        assert_eq!(launch_force(1000.0, 300), MAX_FORCE);
        assert_eq!(launch_force(1.0, 100), MIN_FORCE);
        assert_eq!(launch_force(f32::NAN, 100), MIN_FORCE);
        // A human's threshold (45) knocks with a push on the scale of the game's PushActorAway.
        assert!(launch_force(HUMAN, 100) < 20.0);
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

    /// Settling damps sideways motion and spin, not the fall; it never adds speed.
    #[test]
    fn settling_damps_sideways_and_spin() {
        let v = settle([1.0, -1.0, -3.0], 1.0 / 60.0, true);
        assert!(v[0] < 1.0 && v[0] > 0.8 && v[1] > -1.0);
        assert_eq!(v[2], -3.0);
        let w = settle([2.0, 2.0, 2.0], 1.0 / 60.0, false);
        assert!(w[2] < 2.0);
        assert_eq!(settle([1.0, 1.0, 1.0], 0.0, false), [1.0, 1.0, 1.0]);
        assert!(settle([1.0, 0.0, 0.0], 5.0, true)[0] > 0.0, "dt clamped");
    }
}
