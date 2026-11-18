//! A two-handed weapon fired with one hand (Patrick, 2026-11-15; `ragdoll_core::one_hand`): while
//! an actor's support hand is off its weapon's grip (bracing, or on its way back:
//! `crate::active_ragdoll`), each shot it fires goes wider, by the weapon's effective spread (the
//! game's own spread getter with the spread mods, `0x524be0`) and effective weight (the weapon mods
//! applied, `crate::gunplay::effective_stats`) against the shooter's Strength (actor value 5, which
//! creatures have too), easing back to normal as the hand returns to the grip.
//!
//! **Where:** the active ragdoll's frame says how far off the grip each actor's support hand is
//! ([`set`]); the projectile's creation (`crate::ballistics`'s hook on `Projectile::Create`) turns
//! the shot's yaw and pitch by a random amount within the extra cone ([`widen`]). The player never
//! braces (the active ragdoll leaves the player to the game), so it never applies to the player.

use engine::prelude::*;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Each actor's support hand off its weapon's grip (0 to 1) and when it was said.
static OFF: Mutex<Option<HashMap<u32, (f32, Instant)>>> = Mutex::new(None);

/// How long a share stays good without being said again (s): the active ragdoll says it each
/// frame while it holds the actor.
const FRESH: f32 = 0.25;

/// `actor`'s support hand is `off` its weapon's grip (0 on it, 1 fully off) this frame.
pub fn set(actor: u32, off: f32) {
    let mut guard = OFF.lock().unwrap_or_else(|p| p.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if off > 0.0 {
        map.insert(actor, (off.min(1.0), Instant::now()));
    } else {
        map.remove(&actor);
    }
    if map.len() > 128 {
        map.retain(|_, v| v.1.elapsed().as_secs_f32() < FRESH);
    }
}

/// How far off its weapon's grip `actor`'s support hand is now (0: on it, or not said lately).
pub fn off(actor: u32) -> f32 {
    let guard = OFF.lock().unwrap_or_else(|p| p.into_inner());
    guard
        .as_ref()
        .and_then(|m| m.get(&actor))
        .filter(|v| v.1.elapsed().as_secs_f32() < FRESH)
        .map_or(0.0, |v| v.0)
}

/// Forgets every actor (a load).
pub fn clear() {
    *OFF.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// `Projectile::Create`'s arguments (`009bca60`): the source actor, the weapon, and the shot's yaw
/// and pitch (radians, floats) the game aimed it with.
const ARG_SOURCE: usize = 1;
const ARG_WEAPON: usize = 3;
const ARG_YAW: usize = 7;
const ARG_PITCH: usize = 8;
/// The weapon's spread getter (`thiscall`: weapon, whether a spread mod is attached), degrees.
const SPREAD: u32 = 0x0052_4be0;
/// The Strength actor value.
const AV_STRENGTH: u32 = 5;

/// A shot about to be made (`args`, `Projectile::Create`'s): its yaw and pitch turned within the
/// extra cone one hand gives its shooter, when the shooter's support hand is off a two-handed
/// weapon's grip. Nothing otherwise.
pub fn widen(e: &mut Engine, args: &mut [u32]) {
    if args.len() <= ARG_PITCH {
        return;
    }
    let (shooter, weapon) = (args[ARG_SOURCE], args[ARG_WEAPON]);
    // Only an actor has the actor values read below (a trap or a turret reference fires too).
    if shooter == 0 || weapon == 0 || !jip::commands::is_actor(e, shooter) {
        return;
    }
    let off = off(shooter);
    if off <= 0.0 {
        return;
    }
    let Some(stats) = crate::gunplay::effective_stats(e, shooter, weapon) else {
        return;
    };
    if !stats.is_gun() {
        return;
    }
    let mods = crate::weapon_mods::on_weapon(e, shooter, weapon);
    let spread_mod = u32::from(
        mods.iter()
            .any(|m| m.effect == crate::weapon_mods::Effect::Spread),
    );
    let spread = e.call(SPREAD, &args![weapon, spread_mod]).f64() as f32;
    let strength = engine::actor_values::current(e, shooter, AV_STRENGTH).unwrap_or(f32::NAN);
    let cone = ragdoll_core::one_hand::extra(spread, stats.weight, strength, off).to_radians();
    if cone.is_nan() || cone <= 0.0 {
        return;
    }
    // Uniform over the cone's disc.
    let (u, v) = (random(), random());
    let r = cone * u.sqrt();
    let a = std::f32::consts::TAU * v;
    let turn = |slot: &mut u32, by: f32| {
        let was = f32::from_bits(*slot);
        if was.is_finite() {
            *slot = (was + by).to_bits();
        }
    };
    turn(&mut args[ARG_YAW], r * a.cos());
    turn(&mut args[ARG_PITCH], r * a.sin());
}

/// A number in 0..1 (a xorshift of its own: the shots' spread, not the game's random stream).
fn random() -> f32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static STATE: AtomicU32 = AtomicU32::new(0x9e37_79b9);
    let mut x = STATE.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    STATE.store(x, Ordering::Relaxed);
    (x >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_share_is_kept_while_said_and_forgotten() {
        set(0x1234, 0.6);
        assert_eq!(off(0x1234), 0.6);
        set(0x1234, 0.0);
        assert_eq!(off(0x1234), 0.0);
        assert_eq!(off(0x9999), 0.0);
        let r: Vec<f32> = (0..1000).map(|_| random()).collect();
        assert!(r.iter().all(|x| (0.0..1.0).contains(x)));
        let mean = r.iter().sum::<f32>() / r.len() as f32;
        assert!((mean - 0.5).abs() < 0.05, "{mean}");
    }
}
