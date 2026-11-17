//! Ragdoll hit force (Patrick, 2026-11-14): a body that dies, or is knocked down into its
//! ragdoll, carries the momentum of the shot, the swing or the blast that did it. Part of the
//! weighty combat row ([`crate::gunplay::FIX`]); tuned by `[nv-rs Gunplay] iHitForce` (percent, 0 =
//! the game's own push), also on the Settings menu's Gameplay page. **Not 1:1 with FalloutNV.exe.**
//!
//! **Where the push is applied: the game's own path.** The kill impulse (`008ae000`, on the
//! aggressor with the killing hit's `HitData`) finds the body under the hit (the part the process
//! names) and, through `TESHavokUtilities::AddExplosionImpulse` (`0062b660`), pushes every rigid
//! body of the ragdoll from the impact point along the hit's direction (the projectile's path, or
//! from the weapon to the hit for a swing), more for the parts near the impact within the weapon's
//! impulse radius and by each part's own multiplier, radially with a falloff for an explosion. Its
//! strength is the hit's health damage at `+0x14`. Here that strength is replaced, for the call,
//! by the hit's momentum ([`momentum`]); the hit data is given back afterwards. A knock-down by a
//! blast (`MiddleHighProcess::KnockExplosion`, `0091fee0`) has its strength scaled by `iHitForce`
//! too. Game time is never touched (no slow motion), and the kill camera is not involved.
//!
//! **The momentum** ([`momentum`], in the units of the game's strength: damage points), from
//! [`Shot`]:
//! - a gun (Classic hitscan): its damage per projectile, with its weapon mods, less the target's damage threshold (never below
//!   [`MIN_THROUGH`] of it), one projectile's: each landed pellet is a hit of its own, and a
//!   killing hit carries the momentum the hits just before left stacked in the body
//!   ([`push_body`]: this hit's times the stack over this hit's, `crate::knockdown`'s stack; the
//!   `n^0.75` share of all pellets is gone, 2026-11-14);
//! - a melee or unarmed hit: the weapon's damage made heavier by its weight (`1 + weight /`
//!   [`WEIGHT_PER_DOUBLING`]), longer by its reach, by the attacker's Strength
//!   ([`strength_factor`]) and skill ([`skill_factor`]), [`POWER_ATTACK`] times for a power attack
//!   (recorded where the game makes the melee hit, `HitData::InitializeHitData_ov3`, `009b5170`),
//!   less the damage threshold as for a gun;
//! - an explosion: the game's own (its health damage, the threshold already taken), the falloff
//!   the game's;
//! - a ballistic bullet (Projectile mode, [`crate::ballistics`]): its mass times its speed where
//!   it hit, times its load's transfer, less the threshold by its load's factor, over the pellets
//!   as for a gun;
//! - then times `iHitForce` and `iImpulse` (percent each), at most [`MAX_MOMENTUM`]: bodies are
//!   thrown, never launched.
//!
//! **For a later leg-loss fall** ([`push_body`]): any event with a hit can push the body through
//! the same path (the strength computed the same way, applied by the game's kill impulse).
//!
//! **Upstream's actor push** (#565, `crates/jip/src/world_push.rs`): JIP's `PushActorNoRagdoll`
//! moves a *living* actor through its character controller (its velocity modifier, kept by the
//! ApplyActorVelocity hook) without a ragdoll. A dead or knocked-down body is a ragdoll, which the
//! game pushes through the kill impulse and the blast's knock-down used here; JIP's push is the
//! one to reuse for a hit that only staggers a living actor, and none of its world hooks sits on an
//! address this registers (they are seams inside the translations these wrappers call).

use crate::gunplay::WeaponStats;
use engine::prelude::*;
/// The swing's factors live in `crates/ragdoll-core` (the knockdowns' transfer uses them too).
pub use ragdoll_core::knock::{skill_factor, strength_factor, POWER_ATTACK};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The tuning constants (reported to Patrick; module documentation).
pub const MIN_THROUGH: f32 = 0.2;
pub const WEIGHT_PER_DOUBLING: f32 = 10.0;
pub const MAX_MOMENTUM: f32 = 160.0;
/// The reach the weapon's reach is taken in (a spear's 1.3 pushes 1.3 times as far as a knife's 1).
pub const REACH_RANGE: (f32, f32) = (0.5, 2.0);

/// What made the hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shot {
    /// A gun's projectile: its damage per projectile and its projectiles.
    Gun { damage: f32, projectiles: u32 },
    /// A swing: the weapon's damage, weight and reach (unarmed: the hit's damage, 0, 1), the
    /// attacker's Strength (1 to 10) and attack skill (0 to 100), and whether it was a power attack.
    Melee {
        damage: f32,
        weight: f32,
        reach: f32,
        strength: f32,
        skill: f32,
        power: bool,
    },
    /// A blast: the game's own strength (its health damage).
    Explosion { damage: f32 },
    /// A ballistic bullet ([`crate::ballistics`]): its momentum where it hit (mass times speed,
    /// times its load's transfer, in damage points), the factor the damage threshold counts by
    /// (its load's), and the weapon's projectiles (a shotgun's pellets).
    Bullet {
        momentum: f32,
        threshold: f32,
        projectiles: u32,
    },
}

/// Damage less the target's threshold, never below [`MIN_THROUGH`] of it.
fn through(damage: f32, threshold: f32) -> f32 {
    (damage - threshold.max(0.0))
        .max(damage * MIN_THROUGH)
        .max(0.0)
}

/// The hit's momentum before the multipliers (module documentation), in damage points.
pub fn momentum(shot: Shot, threshold: f32) -> f32 {
    let m = match shot {
        // One projectile's: each landed pellet is its own hit; the pellets that landed just before
        // stack ([`push_body`], `crate::knockdown`'s stack).
        Shot::Gun { damage, .. } => through(damage, threshold),
        Shot::Melee {
            damage,
            weight,
            reach,
            strength,
            skill,
            power,
        } => {
            let swing = damage
                * (1.0 + weight.max(0.0) / WEIGHT_PER_DOUBLING)
                * reach.clamp(REACH_RANGE.0, REACH_RANGE.1)
                * strength_factor(strength)
                * skill_factor(skill)
                * if power { POWER_ATTACK } else { 1.0 };
            through(swing, threshold)
        }
        Shot::Explosion { damage } => damage.max(0.0),
        Shot::Bullet {
            momentum,
            threshold: factor,
            ..
        } => through(momentum, threshold * factor),
    };
    if m.is_finite() {
        m
    } else {
        0.0
    }
}

/// The strength the game's push gets: the momentum times `hit_force` and `impulse` (percent), at
/// most [`MAX_MOMENTUM`].
pub fn push_strength(momentum: f32, hit_force: i32, impulse: i32) -> f32 {
    (momentum * hit_force.max(0) as f32 / 100.0 * impulse.max(0) as f32 / 100.0)
        .clamp(0.0, MAX_MOMENTUM)
}

// --- The game's words. -------------------------------------------------------------------------

/// `HitData`: the aggressor, the target, the source (an explosion for a blast), the attack skill,
/// the health and total damage, the weapon.
const HIT_AGGRESSOR: u32 = 0x00;
const HIT_TARGET: u32 = 0x04;
const HIT_SOURCE: u32 = 0x08;
const HIT_SKILL: u32 = 0x0c;
const HIT_HEALTH_DAMAGE: u32 = 0x14;
const HIT_TOTAL_DAMAGE: u32 = 0x18;
const HIT_WEAPON: u32 = 0x30;
/// `TESObjectREFR::IsExplosion` (virtual `+0x220`: the process lists hand such a reference to
/// `explosion.cpp`'s update, `009ae580`); `+0x224` is `IsProjectile` (`009bd780`), true for
/// every bullet.
const IS_EXPLOSION: u32 = 0x220;
/// `TESObjectWEAP::reach` (the `DNAM` float after the animation multiplier).
const WEAPON_REACH: u32 = 0xfc;
/// Actor values: Strength, damage threshold.
const AV_STRENGTH: u32 = 5;
const AV_DAMAGE_THRESHOLD: u32 = 0x4c;
/// `HitData::InitializeHitData_ov3` (the melee and unarmed hit) and the knock-down by a blast.
pub const MELEE_HIT: u32 = 0x009b_5170;
pub const KNOCK_EXPLOSION: u32 = 0x0091_fee0;

/// An actor's actor value (the `ActorValueOwner` at `+0xa4`, its float getter `+0xc`).
fn actor_value(e: &mut Engine, actor: u32, index: u32) -> f32 {
    if actor == 0 {
        return 0.0;
    }
    let v = e.vcall(actor + 0xa4, 0xc, &args![index]).f64() as f32;
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

/// The power attacks the game made melee hits for lately: (aggressor, target), when.
static POWER: Mutex<Vec<(u32, u32, Instant)>> = Mutex::new(Vec::new());
/// How long a recorded power attack counts for the kill that follows it.
const POWER_MEMORY: Duration = Duration::from_secs(2);

/// Records a melee hit's power attack (from the wrapper over [`MELEE_HIT`]).
pub fn record_melee(aggressor: u32, target: u32, power: bool) {
    let mut list = POWER.lock().unwrap_or_else(|p| p.into_inner());
    list.retain(|&(a, t, at)| (a, t) != (aggressor, target) && at.elapsed() < POWER_MEMORY);
    if power {
        list.push((aggressor, target, Instant::now()));
    }
}

fn was_power(aggressor: u32, target: u32) -> bool {
    POWER
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|&(a, t, at)| (a, t) == (aggressor, target) && at.elapsed() < POWER_MEMORY)
}

/// What made the hit at `hit` (a `HitData`), read from it.
pub fn shot_of(e: &mut Engine, hit: u32, stats: Option<&WeaponStats>) -> Shot {
    let source = e.mem.u32(hit + HIT_SOURCE);
    if source != 0 && e.vcall(source, IS_EXPLOSION, &args![]).bool() {
        return Shot::Explosion {
            damage: e.mem.f32(hit + HIT_HEALTH_DAMAGE),
        };
    }
    if let Some((momentum, threshold)) = crate::ballistics::bullet_momentum(e, source) {
        return Shot::Bullet {
            momentum,
            threshold,
            projectiles: stats.map_or(1, |s| s.projectiles),
        };
    }
    let aggressor = e.mem.u32(hit + HIT_AGGRESSOR);
    let target = e.mem.u32(hit + HIT_TARGET);
    let weapon = e.mem.u32(hit + HIT_WEAPON);
    match stats {
        Some(s) if s.is_gun() => Shot::Gun {
            damage: s.damage,
            projectiles: s.projectiles,
        },
        Some(s) if !s.is_melee() => Shot::Gun {
            // Thrown and placed weapons: their damage, one at a time.
            damage: s.damage,
            projectiles: 1,
        },
        _ => {
            let (damage, weight, reach) = match stats {
                Some(s) => (s.damage, s.weight, e.mem.f32(weapon + WEAPON_REACH)),
                None => (e.mem.f32(hit + HIT_TOTAL_DAMAGE), 0.0, 1.0),
            };
            let skill = e.mem.u32(hit + HIT_SKILL);
            Shot::Melee {
                damage,
                weight,
                reach: if reach.is_finite() { reach } else { 1.0 },
                strength: actor_value(e, aggressor, AV_STRENGTH),
                skill: actor_value(e, aggressor, skill),
                power: was_power(aggressor, target),
            }
        }
    }
}

/// The body the hit at `hit` (a `HitData`) killed, pushed with the hit's momentum through the
/// game's kill impulse (`008ae000`; module documentation). Off (`iHitForce` 0 or the row off):
/// the game's own push, untouched. The hit data is as it was afterwards.
pub fn push_body(e: &mut Engine, aggressor: u32, hit: u32, stats: Option<&WeaponStats>) {
    use engine::units::fallout_ai::actor_p4::fn_008ae000;
    let t = crate::gunplay::tuning();
    if !crate::gunplay::is_on() || hit == 0 || t.hit_force == 0 {
        fn_008ae000(e, Ptr::new(aggressor), Ptr::new(hit));
        return;
    }
    let shot = shot_of(e, hit, stats);
    let target = e.mem.u32(hit + HIT_TARGET);
    let threshold = match shot {
        Shot::Explosion { .. } => 0.0,
        _ => actor_value(e, target, AV_DAMAGE_THRESHOLD),
    };
    let mut m = momentum(shot, threshold);
    // A gun's killing hit carries the momentum the hits just before left stacked in the body (the
    // landed pellets of the same shot, a burst): this hit's times the stack over this hit's.
    if matches!(shot, Shot::Gun { .. } | Shot::Bullet { .. }) {
        let own = crate::knockdown::measure_hit(e, target, hit).p;
        if own > 0.0 {
            let factor = (crate::knockdown::stacked_now(target) + own) / own;
            m *= factor.clamp(1.0, 20.0);
        }
    }
    let strength = push_strength(m, t.hit_force, t.impulse);
    let kept = e.mem.f32(hit + HIT_HEALTH_DAMAGE);
    e.mem.set_f32(hit + HIT_HEALTH_DAMAGE, strength);
    fn_008ae000(e, Ptr::new(aggressor), Ptr::new(hit));
    e.mem.set_f32(hit + HIT_HEALTH_DAMAGE, kept);
}

/// The knock-down by a blast with its strength times `iHitForce` (at most [`MAX_MOMENTUM`]).
pub fn knock_strength(strength: f32) -> f32 {
    let t = crate::gunplay::tuning();
    if !crate::gunplay::is_on() || !strength.is_finite() {
        return strength;
    }
    (strength * t.hit_force.max(0) as f32 / 100.0).min(strength.max(MAX_MOMENTUM))
}

/// Registers the melee hit's record and the knock-down (from `crate::gunplay::install`; the kill
/// impulse itself is the row's own wrapper, which calls [`push_body`]).
pub fn install(e: &mut Engine) {
    use engine::units::fallout_ai::hitdata::hit_data_initialize_hit_data_ov3;
    use engine::units::fallout_ai::middlehighprocess::middle_high_process_knock_explosion;
    e.register(MELEE_HIT, |e, a| {
        record_melee(a[1], a[2], a[4] & 0xff != 0);
        hit_data_initialize_hit_data_ov3(e, Ptr::new(a[0]), a[1], a[2], a[3], a[4]);
        Ret::default()
    });
    e.register(KNOCK_EXPLOSION, |e, a| {
        // An actor knocked down by the knockdowns a moment ago gets no second push.
        // Nor one New Physics has (`HitMe`'s own knock-backs): its fall is its own (M4).
        if crate::gunplay::is_on() && crate::new_physics::owns(a[1]) {
            return Ret::default();
        }
        if crate::gunplay::is_on() && crate::knockdown::recently_knocked(a[1]) {
            crate::knockdown::note_duplicate(e, a[1]);
            return Ret::default();
        }
        let strength = knock_strength(f32::from_bits(a[5]));
        middle_high_process_knock_explosion(e, Ptr::new(a[0]), a[1], a[2], a[3], a[4], strength);
        Ret::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blast is told by `IsExplosion` (`+0x220`), never by `IsProjectile` (`+0x224`), which every
    /// bullet answers: with it, every gunshot counted as a blast and no shot knocked anyone down.
    const _: () = assert!(IS_EXPLOSION == 0x220);

    fn swing(damage: f32, weight: f32, power: bool) -> Shot {
        Shot::Melee {
            damage,
            weight,
            reach: 1.0,
            strength: 5.0,
            skill: 50.0,
            power,
        }
    }

    /// A rifle round pushes by its damage; armor takes its threshold off, never all of it.
    #[test]
    fn a_gun_pushes_by_its_damage_less_armor() {
        let rifle = Shot::Gun {
            damage: 50.0,
            projectiles: 1,
        };
        assert_eq!(momentum(rifle, 0.0), 50.0);
        assert_eq!(momentum(rifle, 20.0), 30.0);
        assert_eq!(momentum(rifle, 100.0), 10.0, "a fifth gets through");
    }

    /// A gun pushes by one projectile's damage: a shotgun's pellets stack as separate hits.
    #[test]
    fn a_gun_pushes_by_one_projectile() {
        let one = |projectiles| {
            momentum(
                Shot::Gun {
                    damage: 7.0,
                    projectiles,
                },
                0.0,
            )
        };
        assert_eq!(one(8), one(1));
    }

    /// A sledgehammer swings harder than a knife, a power attack harder again, Strength and skill
    /// too; reach makes a long weapon push further.
    #[test]
    fn a_swing_pushes_by_weight_strength_skill_and_power() {
        let knife = momentum(swing(10.0, 1.0, false), 0.0);
        let sledge = momentum(swing(35.0, 12.0, false), 0.0);
        let power = momentum(swing(35.0, 12.0, true), 0.0);
        assert!(sledge > 6.0 * knife, "{knife} {sledge}");
        assert!((power / sledge - POWER_ATTACK).abs() < 1e-4);
        assert!(strength_factor(10.0) > strength_factor(5.0));
        assert!((strength_factor(5.0) - 1.0).abs() < 1e-6);
        assert!(skill_factor(100.0) > skill_factor(0.0));
        let melee = |reach: f32| Shot::Melee {
            damage: 20.0,
            weight: 5.0,
            reach,
            strength: 5.0,
            skill: 50.0,
            power: false,
        };
        let (spear, short) = (melee(1.3), melee(1.0));
        assert!(momentum(spear, 0.0) > momentum(short, 0.0));
    }

    /// The push is clamped (bodies are thrown, never launched) and off at 0.
    #[test]
    fn the_push_is_clamped_and_can_be_off() {
        assert_eq!(push_strength(50.0, 100, 100), 50.0);
        assert_eq!(push_strength(50.0, 100, 150), 75.0);
        assert_eq!(push_strength(500.0, 300, 150), MAX_MOMENTUM);
        assert_eq!(push_strength(50.0, 0, 150), 0.0);
        assert_eq!(momentum(Shot::Explosion { damage: 80.0 }, 50.0), 80.0);
        assert_eq!(momentum(Shot::Explosion { damage: f32::NAN }, 0.0), 0.0);
    }
}
