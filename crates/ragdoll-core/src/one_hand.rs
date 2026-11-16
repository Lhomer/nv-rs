//! A two-handed weapon fired with one hand (Patrick, 2026-11-15): while an actor's support hand is
//! off the grip (bracing against a wall, or on its way back: `crate::brace`), the weapon's
//! accuracy is far worse, easing back to normal as the hand returns to the grip. Heavier weapons
//! are worse one-handed (their effective weight, the weapon mods applied); stronger actors steady
//! them better (Strength, which creatures have too); never so steady that one hand is as good as
//! two ([`MOST_STEADY`]).
//!
//! **The numbers.** The weapon's effective spread (degrees: the game's own, the spread mods applied)
//! one-handed is `spread * (1 + k) + k * EXTRA`, `k` the penalty ([`penalty`]): heaviness
//! (`1 + weight / WEIGHT_REF`) times unsteadiness (Strength 5: 1; 10: 0.45; 1: 1.5), held to
//! [`LEAST_PENALTY`]..[`MOST_PENALTY`]. A 9 lb hunting rifle at Strength 5 (k 2.1): its spread three
//! times and nearly 4 degrees more. Between the grip and fully off it, by how far the hand is
//! ([`off_grip`]).

use crate::active::ease;

/// The support hand counts as on the grip within this of it (game units), fully off it from this
/// far (between, eased).
pub const ON_GRIP: f32 = 1.0;
pub const OFF_GRIP: f32 = 12.0;
/// The weight (lb) that doubles the penalty's heaviness.
pub const WEIGHT_REF: f32 = 8.0;
/// Strength 5's steadiness is 1; each point more steadies by this, held to [`MOST_STEADY`] (the
/// strongest still a little less than half as unsteady) and [`LEAST_STEADY`].
pub const STRENGTH_STEP: f32 = 0.11;
pub const MOST_STEADY: f32 = 0.45;
pub const LEAST_STEADY: f32 = 1.5;
/// The penalty at least (even the strongest with the lightest two-hander) and at most.
pub const LEAST_PENALTY: f32 = 0.75;
pub const MOST_PENALTY: f32 = 6.0;
/// The spread one hand adds per unit of penalty (degrees), whatever the weapon's own.
pub const EXTRA: f32 = 1.8;

/// How far off the grip the support hand is (0 on it, 1 fully off), from its wrist's distance to
/// the grip (game units).
pub fn off_grip(distance: f32) -> f32 {
    if !distance.is_finite() {
        return 1.0;
    }
    ease((distance - ON_GRIP) / (OFF_GRIP - ON_GRIP))
}

/// How unsteady an actor of `strength` (1 to 10; anything else as 5) holds a weapon in one hand.
pub fn unsteadiness(strength: f32) -> f32 {
    let s = if strength.is_finite() && strength > 0.0 {
        strength.min(10.0)
    } else {
        5.0
    };
    (1.0 - (s - 5.0) * STRENGTH_STEP).clamp(MOST_STEADY, LEAST_STEADY)
}

/// How much harder a weapon of `weight` (lb, effective) is to hold in one hand.
pub fn heaviness(weight: f32) -> f32 {
    let w = if weight.is_finite() {
        weight.max(0.0)
    } else {
        0.0
    };
    1.0 + w.min(60.0) / WEIGHT_REF
}

/// The one-handed penalty for a weapon of `weight` held by an actor of `strength`.
pub fn penalty(weight: f32, strength: f32) -> f32 {
    (heaviness(weight) * unsteadiness(strength)).clamp(LEAST_PENALTY, MOST_PENALTY)
}

/// The weapon's spread (degrees) fired with the support hand `off` the grip (0 to 1): its
/// effective spread `spread` (the game's, the spread mods applied) at 0, the one-handed spread at
/// 1, eased between.
pub fn spread(spread: f32, weight: f32, strength: f32, off: f32) -> f32 {
    let base = if spread.is_finite() {
        spread.max(0.0)
    } else {
        0.0
    };
    let k = penalty(weight, strength);
    let one = base * (1.0 + k) + k * EXTRA;
    base + (one - base) * off.clamp(0.0, 1.0)
}

/// The spread one hand adds to a shot (degrees, a cone's half-angle on top of the game's own).
pub fn extra(spread_deg: f32, weight: f32, strength: f32, off: f32) -> f32 {
    let base = if spread_deg.is_finite() {
        spread_deg.max(0.0)
    } else {
        0.0
    };
    (spread(base, weight, strength, off) - base).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_hand_is_far_worse_heavier_worse_stronger_better() {
        // A hunting rifle (9 lb, 0.5 degrees) at Strength 5: on the grip its own; off it, much
        // worse.
        assert_eq!(spread(0.5, 9.0, 5.0, 0.0), 0.5);
        let off = spread(0.5, 9.0, 5.0, 1.0);
        assert!(off > 0.5 * 4.0, "{off}");
        // Heavier is worse, stronger is better.
        assert!(spread(0.5, 20.0, 5.0, 1.0) > off);
        assert!(spread(0.5, 9.0, 10.0, 1.0) < off);
        assert!(spread(0.5, 9.0, 1.0, 1.0) > off);
        // The strongest with the lightest two-hander still well off its own.
        let best = spread(0.5, 0.0, 10.0, 1.0);
        assert!(best >= 0.5 + LEAST_PENALTY * EXTRA - 1e-4, "{best}");
        assert!(best > 0.5 * 2.0);
        // On the grip at ON_GRIP, fully off at OFF_GRIP, eased between and never back up.
        assert_eq!(off_grip(0.5), 0.0);
        assert_eq!(off_grip(30.0), 1.0);
        let mut last = 0.0;
        for i in 0..=60 {
            let o = off_grip(i as f32 * 0.25);
            assert!(o >= last - 1e-6);
            last = o;
        }
        assert_eq!(unsteadiness(f32::NAN), 1.0);
        assert_eq!(extra(0.5, 9.0, 5.0, 0.0), 0.0);
    }
}
