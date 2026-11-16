//! The numbers of nv-rs's own perks (`host::perks`: five perk records nv-rs writes as its own
//! plugin, `nv-rs Perks.esp`), shared by the host and the lab: what each changes, with no game.
//!
//! - **Stopping Power** ([`STOPPING_POWER_PUSH`], [`STOPPING_POWER_STAGGER`]): the player's hits
//!   push 1.35 times as hard into the active ragdoll (the flinch and the catch steps), and stagger
//!   (interrupt an attack) from 75% of the share of the topple speed they did; the damage, the
//!   stack and the knock-down are the hit's own.
//! - **Hot Loads** ([`HOT_LOADS`]): every gun the player fires leaves the muzzle 10% faster (its
//!   own factor of the velocity), so its drop, its push and its penetration follow; not its damage.
//! - **Bowling Ball** ([`bowling_push`]): a body the player knocked over, flying into another
//!   actor, gives it a push of its own momentum's share.
//! - **Light Touch+** ([`light_touch_chance`]): the pickpocket chance's value term half its
//!   penalty, plus 10.
//! - **Scavenger's Instinct** ([`beats`]): loot that beats what the player has equipped in its slot.

use crate::knock::Reaction;

/// Stopping Power: the push into the active ragdoll, and the share of the stagger threshold.
pub const STOPPING_POWER_PUSH: f32 = 1.35;
pub const STOPPING_POWER_STAGGER: f32 = 0.75;

/// Hot Loads: the muzzle velocity's factor.
pub const HOT_LOADS: f32 = 1.10;

/// Bowling Ball: the share of the flying body's momentum the actor it hits takes, how near (game
/// units, pelvis to pelvis) counts as hitting it, how long after the knock-down a body flies, and
/// the least speed (m/s) that is flying.
pub const BOWLING_SHARE: f32 = 0.25;
pub const BOWLING_REACH: f32 = 48.0;
pub const BOWLING_FOR: f32 = 1.5;
pub const BOWLING_LEAST_SPEED: f32 = 1.0;

/// Light Touch+: the share of the value term's penalty left, and the flat chance added (points).
pub const LIGHT_TOUCH_VALUE_SHARE: f32 = 0.5;
pub const LIGHT_TOUCH_FLAT: f32 = 10.0;

/// The push a hit gives the active ragdoll (N s): `p`, Stopping Power's share more with it.
pub fn stopping_push(p: f32, stopping_power: bool) -> f32 {
    if stopping_power {
        p * STOPPING_POWER_PUSH
    } else {
        p
    }
}

/// The reaction to a hit of `speed` against `topple` (`crate::knock::reaction`), the stagger from
/// Stopping Power's lower share with it.
pub fn reaction(speed: f32, topple: f32, stopping_power: bool) -> Reaction {
    let base = crate::knock::reaction(speed, topple);
    if !stopping_power || base != Reaction::None {
        return base;
    }
    let share = crate::tuning::get().stagger_share * STOPPING_POWER_STAGGER;
    if speed.is_finite() && speed > 0.0 && speed >= topple * share {
        Reaction::Stagger
    } else {
        Reaction::None
    }
}

/// A muzzle velocity (or its factor) with Hot Loads.
pub fn hot_loads(velocity: f32, hot_loads: bool) -> f32 {
    if hot_loads {
        velocity * HOT_LOADS
    } else {
        velocity
    }
}

/// Bowling Ball: the push (N s) a body of `mass` (kg) flying at `speed` (m/s) gives the actor it
/// hits; none below [`BOWLING_LEAST_SPEED`].
pub fn bowling_push(mass: f32, speed: f32) -> f32 {
    if !(mass.is_finite() && speed.is_finite()) || speed < BOWLING_LEAST_SPEED || mass <= 0.0 {
        return 0.0;
    }
    mass * speed * BOWLING_SHARE
}

/// The pickpocket chance (`00643400`'s three terms, then its limits) with Light Touch+: `player`
/// and `target` terms as the game makes them, the value term `value_scale * value + value_add`
/// with half the penalty (`value_scale * value * 0.5`), each term stored as a float as the game
/// stores it, 10 more, the sum truncated, then `low` and `high`.
pub fn light_touch_chance(
    player_term: f64,
    target_term: f64,
    value: f32,
    value_scale: f64,
    value_add: f64,
    low: f64,
    high: f64,
) -> i32 {
    let float = |x: f64| f64::from(x as f32);
    let value_term =
        float(value_scale * f64::from(value) * f64::from(LIGHT_TOUCH_VALUE_SHARE) + value_add);
    let sum = float(player_term) + float(target_term) + value_term + f64::from(LIGHT_TOUCH_FLAT);
    let mut chance = sum as i32;
    if f64::from(chance) < low {
        chance = low as i32;
    } else if f64::from(chance) > high {
        chance = high as i32;
    }
    chance
}

/// What an item is worth to the player against what they have equipped in its slot: a weapon's
/// force (`gunplay::force`: its damage of all its projectiles, a melee weapon's made heavier by its
/// weight), apparel's damage threshold then resistance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Score {
    Weapon(f32),
    Apparel { dt: f32, dr: f32 },
}

impl Score {
    fn above(self, other: Score) -> Option<bool> {
        match (self, other) {
            (Score::Weapon(a), Score::Weapon(b)) => Some(a > b + 1e-3),
            (Score::Apparel { dt: a, dr: ar }, Score::Apparel { dt: b, dr: br }) => {
                Some(a > b + 1e-3 || ((a - b).abs() <= 1e-3 && ar > br + 1e-3))
            }
            _ => None,
        }
    }

    fn any(self) -> bool {
        match self {
            Score::Weapon(f) => f > 0.0,
            Score::Apparel { dt, dr } => dt > 0.0 || dr > 0.0,
        }
    }
}

/// Scavenger's Instinct: whether `item` beats every equipped item it would replace (`equipped`);
/// with nothing equipped in its slot, whether it is worth anything at all.
pub fn beats(item: Score, equipped: &[Score]) -> bool {
    let mut compared = false;
    for &worn in equipped {
        match item.above(worn) {
            Some(true) => compared = true,
            Some(false) => return false,
            None => {}
        }
    }
    compared || (equipped.iter().all(|w| item.above(*w).is_none()) && item.any())
}

/// Scavenger's Instinct's order: the rows that beat the equipped ones first, each group in its
/// own order (a stable sort of `better`, the rows' flags).
pub fn better_first<T>(rows: &mut [T], better: impl Fn(&T) -> bool) {
    rows.sort_by_key(|r| !better(r));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stopping Power pushes 1.35 times as hard and staggers from 75% of the stagger share; a
    /// knock-down is the hit's own.
    #[test]
    fn stopping_power_pushes_harder_and_staggers_sooner() {
        assert_eq!(stopping_push(40.0, false), 40.0);
        assert!((stopping_push(40.0, true) - 54.0).abs() < 1e-4);
        let topple = 1.4;
        let share = crate::tuning::get().stagger_share;
        let between = topple * share * 0.9;
        assert_eq!(reaction(between, topple, false), Reaction::None);
        assert_eq!(reaction(between, topple, true), Reaction::Stagger);
        let below = topple * share * STOPPING_POWER_STAGGER * 0.9;
        assert_eq!(reaction(below, topple, true), Reaction::None);
        assert_eq!(reaction(topple, topple, true), Reaction::Knockdown);
        assert_eq!(reaction(topple * 0.99, topple, true), Reaction::Stagger);
    }

    #[test]
    fn hot_loads_is_ten_percent() {
        assert_eq!(hot_loads(400.0, false), 400.0);
        assert!((hot_loads(400.0, true) - 440.0).abs() < 1e-3);
    }

    /// A body knocked over pushes the actor it flies into by its momentum's share; a slow one none.
    #[test]
    fn a_flying_body_pushes_what_it_hits() {
        assert_eq!(bowling_push(80.0, 0.5), 0.0);
        assert!((bowling_push(80.0, 3.0) - 60.0).abs() < 1e-3);
        assert!(bowling_push(80.0, 6.0) > bowling_push(80.0, 3.0));
        assert_eq!(bowling_push(f32::NAN, 3.0), 0.0);
    }

    /// Light Touch+: the value's penalty halved, 10 added, the game's limits kept.
    #[test]
    fn light_touch_halves_the_value_penalty_and_adds_ten() {
        // A skill term of 40, the target's -10, a value of 200 at -0.1 a cap, limits 5..95.
        let plain = 40.0 - 10.0 + (-0.1 * 200.0);
        let with = light_touch_chance(40.0, -10.0, 200.0, -0.1, 0.0, 5.0, 95.0);
        assert_eq!(plain as i32, 10);
        assert_eq!(with, 30); // 40 - 10 - 10 + 10
        assert_eq!(light_touch_chance(90.0, 0.0, 0.0, -0.1, 0.0, 5.0, 95.0), 95);
        assert_eq!(
            light_touch_chance(0.0, -50.0, 1000.0, -0.1, 0.0, 5.0, 95.0),
            5
        );
    }

    /// Scavenger's Instinct: a better weapon or apparel beats what it would replace; a worse one,
    /// or one worse than any of the equipped it replaces, does not; an empty slot is beaten by
    /// anything worth something; the better rows go first, each group in its order.
    #[test]
    fn loot_that_beats_the_equipped_goes_first() {
        let gun = |f| Score::Weapon(f);
        let wear = |dt, dr| Score::Apparel { dt, dr };
        assert!(beats(gun(30.0), &[gun(20.0)]));
        assert!(!beats(gun(20.0), &[gun(20.0)]));
        assert!(!beats(gun(10.0), &[gun(20.0)]));
        assert!(beats(wear(12.0, 0.0), &[wear(10.0, 30.0)]));
        assert!(beats(wear(10.0, 31.0), &[wear(10.0, 30.0)]));
        assert!(!beats(wear(12.0, 0.0), &[wear(10.0, 0.0), wear(15.0, 0.0)]));
        assert!(beats(gun(5.0), &[]));
        assert!(!beats(wear(0.0, 0.0), &[]));
        let mut rows = vec![("a", false), ("b", true), ("c", false), ("d", true)];
        better_first(&mut rows, |r| r.1);
        assert_eq!(rows.iter().map(|r| r.0).collect::<String>(), "bdac");
    }
}
