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
//!   its character controller are set at a clear spot by the body with no velocity (the game's
//!   `PathingLocation` moved onto the closest navmesh triangle and checked clear [`SPOT_RADIUS`]
//!   around, within [`SPOT_DISTANCE`]; else, or for a body that never lay down, where it was
//!   knocked from), and the game's get-up goes on from there.
//! - **Once:** an actor knocked down in the last [`KNOCK_COOLDOWN`] or got up in the last
//!   [`UP_COOLDOWN`] is not knocked again, the game's own blast knock-down
//!   ([`crate::hit_force::KNOCK_EXPLOSION`]) adds no second push to an actor knocked here a moment
//!   ago ([`recently_knocked`]), and JIP's stagger push left on the character controller is
//!   cleared before the ragdoll takes over. While it is down, each bone's speed is held to
//!   [`MAX_BONE_SPEED`] and its spin to [`MAX_BONE_SPIN`] for [`FLIGHT`], and then a bone slower
//!   than [`SETTLE_SPEED`] has its sideways motion and spin damped ([`SETTLE_DAMPING`] a second).
//!   Every launch, clamp, guard and get-up decision is a `Knockdown:` log line.
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
/// The game's own knock-down (`KnockExplosion`) only puts the body into its ragdoll: its push is
/// this small. The launch is the knockdowns' own, given to the limp ragdoll ([`frame`]): game
/// units a second per point of momentum, between [`MIN_LAUNCH`] and [`MAX_LAUNCH`] (about 2 to
/// 10 m/s), lifted by [`LIFT`].
pub const KNOCK_STRENGTH: f32 = 2.0;
pub const LAUNCH_PER_POINT: f32 = 6.0;
pub const MIN_LAUNCH: f32 = 150.0;
pub const MAX_LAUNCH: f32 = 700.0;
pub const LIFT: f32 = 0.3;
/// The launch is given again (at most this many times) when the next frame shows less than this
/// share of it left (the ragdoll's own start took it back).
pub const LAUNCH_TRIES: u8 = 2;
pub const LAUNCH_KEPT: f32 = 0.4;
pub const STAGGER_PER_POINT: f32 = 2.0;
pub const MAX_STAGGER: f32 = 200.0;
pub const ORIGIN_BACK: f32 = 400.0;
/// No second knock-down this soon after one, nor this soon after getting up.
pub const KNOCK_COOLDOWN: Duration = Duration::from_millis(2500);
pub const UP_COOLDOWN: Duration = Duration::from_millis(1500);
/// A bone's speed (game units a second, turned into Havok's by the game's scale, [`hk_scale`]) and
/// spin (radians a second) at most while it flies (outliers only: the launch and a fall stay
/// well below), how long that is held, and the settling: below this speed, sideways motion and
/// spin lose this share a second.
pub const MAX_BONE_SPEED: f32 = 1400.0;
pub const MAX_BONE_SPIN: f32 = 20.0;
pub const FLIGHT: Duration = Duration::from_millis(1500);
pub const SETTLE_SPEED: f32 = 12.0;
pub const SETTLE_DAMPING: f32 = 6.0;
/// The get-up waits ([`hold_get_up`]) until every bone is slower than [`STILL_SPEED`] for
/// [`STILL_FRAMES`] frames in a row and the body lies (its bones within [`LYING_HEIGHT`] of each
/// other in height), at most [`HOLD_MAX`]; then the actor stands at a clear navmesh spot within
/// [`SPOT_DISTANCE`] of the body ([`SPOT_RADIUS`] clear around it), else where it was knocked from.
pub const STILL_SPEED: f32 = 30.0;
pub const STILL_FRAMES: u32 = 20;
pub const LYING_HEIGHT: f32 = 70.0;
pub const HOLD_MAX: Duration = Duration::from_secs(8);
pub const SPOT_DISTANCE: f32 = 200.0;
pub const SPOT_RADIUS: f32 = 25.0;
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

/// The launch's speed (game units a second) for a momentum `m`: times [`LAUNCH_PER_POINT`] and
/// `hit_force` percent, between [`MIN_LAUNCH`] and [`MAX_LAUNCH`].
pub fn launch_speed(m: f32, hit_force: i32) -> f32 {
    let f = m * LAUNCH_PER_POINT * hit_force.max(0) as f32 / 100.0;
    if f.is_nan() {
        MIN_LAUNCH
    } else {
        f.clamp(MIN_LAUNCH, MAX_LAUNCH)
    }
}

/// The launch's direction: the hit's, lifted by [`LIFT`], of unit length.
pub fn launch_direction(direction: [f32; 3]) -> [f32; 3] {
    let d = [direction[0], direction[1], direction[2] + LIFT];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if l.is_nan() || l < 1e-3 {
        [0.0, 0.0, 1.0]
    } else {
        d.map(|x| x / l)
    }
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
    let strength = KNOCK_STRENGTH;
    let speed = launch_speed(m, t.hit_force);
    let origin = [0, 1, 2].map(|i| point[i] - direction[i] * ORIGIN_BACK);
    let stood = position(e, actor);
    let start = stood.map_or(origin[2], |p| p[2]);
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
    let wanted = m * LAUNCH_PER_POINT * t.hit_force.max(0) as f32 / 100.0;
    if wanted > MAX_LAUNCH || wanted < MIN_LAUNCH {
        note(format!(
            "{who}: clamp: launch {wanted:.0} units/s held to {speed:.0} ({MIN_LAUNCH}-{MAX_LAUNCH})"
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
    let launch = launch_direction(direction);
    LAUNCHES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(Launch {
            actor,
            direction: launch,
            speed,
            frames: 0,
            tries: 0,
            applied: false,
        });
    {
        let mut ups = GET_UPS.lock().unwrap_or_else(|p| p.into_inner());
        ups.retain(|g| g.actor != actor);
        ups.push(GetUp {
            actor,
            start: stood.unwrap_or(point),
            since: Instant::now(),
            lying_since: None,
            still_frames: 0,
            held_said: false,
            allowed: false,
        });
    }
    note(format!(
        "{who}: launch {speed:.0} units/s along ({:.2}, {:.2}, {:.2}) once the ragdoll is limp",
        launch[0], launch[1], launch[2]
    ));
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
    launches(e);
    get_ups(e, dt);
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

/// A knock-down's launch, given once its ragdoll is limp.
#[derive(Clone, Copy, Debug)]
struct Launch {
    actor: u32,
    direction: [f32; 3],
    /// Game units a second.
    speed: f32,
    frames: u32,
    tries: u8,
    applied: bool,
}

static LAUNCHES: Mutex<Vec<Launch>> = Mutex::new(Vec::new());

/// Once a frame: each pending launch given to its ragdoll the frame after it went limp (knock
/// state 2: the game's own set-up of the ragdoll is done, so it is not wiped), every bone the same
/// velocity added along the hit; checked the next frame and given again when the ragdoll took it
/// back ([`LAUNCH_TRIES`]).
fn launches(e: &mut Engine) {
    let pending: Vec<Launch> =
        std::mem::take(&mut *LAUNCHES.lock().unwrap_or_else(|p| p.into_inner()));
    if pending.is_empty() {
        return;
    }
    let scale = hk_scale(e);
    let mut keep = Vec::new();
    for mut l in pending {
        let process = e.mem.u32(l.actor + PROCESS);
        if process == 0 || matches!(e.mem.u32(l.actor + LIFE_STATE), 1 | 2) {
            continue;
        }
        let state = e.mem.u8(process + KNOCK_STATE) as i8;
        l.frames += 1;
        if state != 2 {
            if l.frames > 30 {
                note(format!(
                    "{}: launch dropped: the ragdoll never went limp (knock state {state})",
                    describe(e, l.actor)
                ));
            } else {
                keep.push(l);
            }
            continue;
        }
        if l.frames < 2 {
            keep.push(l);
            continue;
        }
        let add = l.direction.map(|x| x * l.speed / scale);
        let bones = bodies(e, l.actor);
        if !l.applied {
            for &(_, body) in &bones {
                if let Some(v) = linear(e, body) {
                    let nv = [v[0] + add[0], v[1] + add[1], v[2] + add[2]];
                    set_velocity(e, SET_LINEAR_VELOCITY, body, nv);
                }
            }
            l.applied = true;
            note(format!(
                "{}: impulse applied: {} bones +{:.0} units/s ({:.1} m/s) along ({:.2}, {:.2}, \
                 {:.2}), Havok scale {scale}",
                describe(e, l.actor),
                bones.len(),
                l.speed,
                l.speed / 70.0,
                l.direction[0],
                l.direction[1],
                l.direction[2]
            ));
            keep.push(l);
            continue;
        }
        // The frame after: how much of it the bones kept, along the launch.
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
            l.applied = false;
            note(format!(
                "{}: duplicate guard: the launch was taken back ({kept:.0} of {:.0} units/s left): \
                 given again ({}/{LAUNCH_TRIES})",
                describe(e, l.actor),
                l.speed,
                l.tries
            ));
            keep.push(l);
        } else {
            note(format!(
                "{}: the launch holds: {kept:.0} units/s along it",
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
    /// Where it stood when knocked down (a clear spot: the fallback).
    start: [f32; 3],
    since: Instant,
    /// Since it lay down (knock state 1 or 3) and the get-up began to be held.
    lying_since: Option<Instant>,
    still_frames: u32,
    held_said: bool,
    /// Placed at its spot: the game's get-up may run.
    allowed: bool,
}

static GET_UPS: Mutex<Vec<GetUp>> = Mutex::new(Vec::new());

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
/// controller set at a clear spot by the body with no velocity ([`clear_spot`], else where it was
/// knocked from).
fn get_ups(e: &mut Engine, dt: f32) {
    let list: Vec<GetUp> = GET_UPS.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if list.is_empty() {
        return;
    }
    let scale = hk_scale(e);
    let mut keep = Vec::new();
    for mut g in list {
        let process = e.mem.u32(g.actor + PROCESS);
        if process == 0
            || matches!(e.mem.u32(g.actor + LIFE_STATE), 1 | 2)
            || g.since.elapsed() > Duration::from_secs(30)
        {
            continue;
        }
        let state = e.mem.u8(process + KNOCK_STATE) as i8;
        if state == 0 && g.since.elapsed() > Duration::from_millis(500) {
            continue; // up again
        }
        if matches!(state, 1 | 2) {
            hold_bones(e, g.actor, g.since.elapsed() < FLIGHT, dt, scale);
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
        let lying = height <= LYING_HEIGHT && !bones.is_empty();
        let spot = if lying {
            clear_spot(e, g.actor, [centre[0], centre[1], low])
        } else {
            None
        };
        let (at, why) = match spot {
            Some(at) => (at, "a clear navmesh spot by the body"),
            None => (
                g.start,
                if lying {
                    "where it was knocked from (no clear spot by the body)"
                } else {
                    "where it was knocked from (it never lay down)"
                },
            ),
        };
        place(e, g.actor, at);
        g.allowed = true;
        note(format!(
            "{}: get-up allowed after {:.1} s ({}): set at ({:.0}, {:.0}, {:.0}), {why}, no velocity",
            describe(e, g.actor),
            held.as_secs_f32(),
            if held >= HOLD_MAX { "held too long" } else { "still and lying" },
            at[0],
            at[1],
            at[2]
        ));
        keep.push(g);
    }
    *GET_UPS.lock().unwrap_or_else(|p| p.into_inner()) = keep;
}

/// A clear navmesh spot by `near` (the body's centre at the floor's height): the game's
/// `PathingLocation` of the point in the actor's cell, moved onto its closest navmesh triangle
/// (`ResolveToClosestNavmeshAndTriangle`, `006ddc00`) and valid with [`SPOT_RADIUS`] clear around
/// it (`IsValidLocation`, `006ded70`: line of sight four ways), within [`SPOT_DISTANCE`].
fn clear_spot(e: &mut Engine, actor: u32, near: [f32; 3]) -> Option<[f32; 3]> {
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
        let away = ((at[0] - near[0]).powi(2) + (at[1] - near[1]).powi(2)).sqrt();
        if !at.iter().all(|x| x.is_finite()) || away > SPOT_DISTANCE {
            return None;
        }
        if !e
            .call(IS_VALID_LOCATION, &args![location, SPOT_RADIUS])
            .bool()
        {
            return None;
        }
        Some([at[0], at[1], near[2].max(at[2])])
    })
}

/// The actor's reference and character controller set at `at`, the controller's velocity zero.
fn place(e: &mut Engine, actor: u32, at: [f32; 3]) {
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

/// A knocked-down actor's bones: while it flies, held to [`MAX_BONE_SPEED`] and
/// [`MAX_BONE_SPIN`]; afterwards, a slow bone's sideways motion and spin damped ([`settle`]).
/// Speeds in game units (`scale` game units a Havok unit).
fn hold_bones(e: &mut Engine, actor: u32, flying: bool, dt: f32, scale: f32) {
    let mut clamped = 0;
    let mut fastest = 0.0f32;
    let (most, settle_below) = (MAX_BONE_SPEED / scale, SETTLE_SPEED / scale);
    for (_, body) in bodies(e, actor) {
        let at = e.call(LINEAR_VELOCITY, &args![body]).u32();
        let spin_at = e.call(ANGULAR_VELOCITY, &args![body]).u32();
        if at == 0 || spin_at == 0 {
            continue;
        }
        let v = read3(e, at);
        let w = read3(e, spin_at);
        let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if flying {
            let (nv, a) = clamp_speed(v, most);
            let (nw, b) = clamp_speed(w, MAX_BONE_SPIN);
            if a {
                set_velocity(e, SET_LINEAR_VELOCITY, body, nv);
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
        } else if speed < settle_below && dt > 0.0 {
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
        assert_eq!(launch_speed(50.0, 100), 300.0);
        assert_eq!(launch_speed(50.0, 200), 600.0);
        assert_eq!(launch_speed(1000.0, 300), MAX_LAUNCH);
        assert_eq!(launch_speed(1.0, 100), MIN_LAUNCH);
        assert_eq!(launch_speed(f32::NAN, 100), MIN_LAUNCH);
        // A human's threshold (45): about 4 m/s; an anti-materiel rifle's (100): about 8.6 m/s.
        assert!((200.0..350.0).contains(&launch_speed(HUMAN, 100)));
        assert!(launch_speed(100.0, 100) > 500.0);
        // Along the hit, a little up, of unit length.
        let d = launch_direction([1.0, 0.0, 0.0]);
        assert!(d[0] > 0.9 && d[2] > 0.2);
        assert!(((d[0] * d[0] + d[2] * d[2]).sqrt() - 1.0).abs() < 1e-5);
        assert_eq!(launch_direction([f32::NAN, 0.0, 0.0]), [0.0, 0.0, 1.0]);
    }

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
