//! The knockdowns' pure parts (Patrick, 2026-11-14): the bodies (mass, centre of mass, stance), the
//! momentum a hit transfers, the speed that topples a standing body, the reaction, the impact
//! stack's fading and the launch. The host's `crate::knockdown` re-exports this module and adds the
//! game's side (`Actor::HitMe`, the ragdoll, the get-up); its documentation explains the physics.
//! The swing's factors (`crate::hit_force` in the host) are here too, as the transfer uses them.
//!
//! The constants are the game's tuning; the functions read them through [`crate::tuning::get`].

use crate::rounds::Round;
use crate::tuning;

/// The tuning constants (module documentation).
///
/// **Bodies** ([`figures`]): mass (kg), centre of mass height (m), half its stance (m), at scale 1.
pub const HUMAN: Figure = Figure::new(80.0, 1.0, 0.15);
pub const SUPER_MUTANT: Figure = Figure::new(250.0, 1.2, 0.25);
/// By creature type (`0059f3a0`): animal (dogs, coyotes), mutated animal (bighorners, bears),
/// mutated insect (ants, radscorpions, cazadores), abomination (deathclaws), super mutant, feral
/// ghoul, robot, giant.
pub const CREATURE: [Figure; 8] = [
    Figure::new(30.0, 0.4, 0.2),
    Figure::new(150.0, 0.8, 0.4),
    Figure::new(25.0, 0.3, 0.4),
    Figure::new(400.0, 1.4, 0.5),
    Figure::new(250.0, 1.2, 0.25),
    Figure::new(55.0, 0.9, 0.15),
    Figure::new(200.0, 1.0, 0.3),
    Figure::new(1500.0, 2.0, 0.8),
];
/// Gravity (m/s2) and how much faster than the bare tip-over speed a standing body can be pushed
/// before it can no longer step to catch itself.
pub const GRAVITY: f32 = 9.81;
pub const RECOVERY: f32 = 3.0;
/// **Momentum transfer**: a bullet's real momentum topples nobody (a .50 round: 38 N s on 80 kg is
/// 0.5 m/s), so the rounds' momentum is taken [`BULLET_FEEL`] times, as films and games show it; a
/// swing ([`ARM_MASS`] of arm behind the weapon's mass at [`SWING_SPEED`], by Strength, skill,
/// reach and power attack, leaving [`MELEE_SHARE`] of it) and a blast ([`BLAST_PER_POINT`] N s per
/// point of the game's blast damage at that distance, its falloff included) as they are. Armour
/// takes `1 / (1 + DT / ARMOUR_DT)` of a round's or a swing's (DT by the load's factor).
/// A gun of no known calibre: [`DAMAGE_MOMENTUM`] N s per point of damage, as a standard round.
pub const BULLET_FEEL: f32 = 15.0;
pub const ARM_MASS: f32 = 3.0;
pub const SWING_SPEED: f32 = 8.0;
pub const MELEE_SHARE: f32 = 0.8;
pub const BLAST_PER_POINT: f32 = 3.5;
pub const ARMOUR_DT: f32 = 40.0;
pub const DAMAGE_MOMENTUM: f32 = 0.3;
pub const POUND: f32 = 0.4536;
/// The launch at most (m/s; outliers), and game units a metre.
pub const MAX_LAUNCH: f32 = 12.0;
pub const UNITS_PER_METRE: f32 = crate::UNITS_PER_METRE;
pub const STAGGER_SHARE: f32 = 0.4;
/// The launch's lift: the launch direction is the hit's with this added upward.
pub const LIFT: f32 = 0.3;
/// A swing's power attack: this many times the swing's speed.
pub const POWER_ATTACK: f32 = 1.5;
/// The get-up waits until every bone is slower than [`STILL_SPEED`] (units a second) for
/// [`STILL_FRAMES`] frames in a row and the body lies (its bones within [`LYING_HEIGHT`] units of
/// each other in height), at most [`HOLD_SECONDS`].
pub const STILL_SPEED: f32 = 30.0;
pub const STILL_FRAMES: u32 = 20;
pub const LYING_HEIGHT: f32 = 70.0;
/// A still ragdoll lies ([`lying`]) only with its pelvis within [`LYING_PELVIS`] of the floor
/// (game units, at scale 1) and its spine-to-head line within [`LYING_TILT`] of the horizontal
/// (degrees). [`LYING_HEIGHT`]'s span alone let a kneeling slump get up: the game's get-up from
/// lying yanked it flat in 4 frames (Patrick's 08-21-49 clip, the pelvis 23 units over the floor).
pub const LYING_PELVIS: f32 = 12.0;
pub const LYING_TILT: f32 = 35.0;
/// A still body that does not lie is nudged along its lean to finish toppling ([`lean`]): its
/// upper body this fast (m/s), every [`NUDGE_EVERY`] seconds, at most [`NUDGES`] times; then it
/// waits up to the hold's most.
pub const NUDGE_SPEED: f32 = 0.8;
pub const NUDGE_EVERY: f32 = 0.6;
pub const NUDGES: u32 = 3;

/// Whether a still ragdoll lies: its pelvis `pelvis_over_floor` game units over the floor (at
/// `scale`), the line from its spine `spine` to its head `head` within [`LYING_TILT`] of level.
pub fn lying(pelvis_over_floor: f32, spine: [f32; 3], head: [f32; 3], scale: f32) -> bool {
    let d = [head[0] - spine[0], head[1] - spine[1], head[2] - spine[2]];
    let flat = (d[0] * d[0] + d[1] * d[1]).sqrt();
    let tilt = d[2].abs().atan2(flat).to_degrees();
    pelvis_over_floor <= LYING_PELVIS * scale.max(0.25) && tilt <= LYING_TILT
}

/// The way a body that does not lie leans (flat, unit: from its spine toward its head), along
/// which it is nudged to finish toppling; straight up, the way it faces `facing`.
pub fn lean(spine: [f32; 3], head: [f32; 3], facing: [f32; 3]) -> [f32; 3] {
    let d = [head[0] - spine[0], head[1] - spine[1]];
    let l = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if l > 2.0 {
        return [d[0] / l, d[1] / l, 0.0];
    }
    let l = (facing[0] * facing[0] + facing[1] * facing[1]).sqrt();
    if l > 1e-3 {
        [facing[0] / l, facing[1] / l, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    }
}
pub const HOLD_SECONDS: f32 = 8.0;

/// Strength's share of a swing: 0.68 at 1, 1.0 at 5, 1.4 at 10.
pub fn strength_factor(strength: f32) -> f32 {
    0.6 + 0.08 * strength.clamp(1.0, 10.0)
}

/// The attack skill's share of a swing: 0.8 at 0, 1.2 at 100.
pub fn skill_factor(skill: f32) -> f32 {
    0.8 + skill.clamp(0.0, 100.0) / 250.0
}

/// What kind of body takes the hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Body {
    Human,
    SuperMutant,
    /// A creature of the game's type (0 to 7; anything else counts as an animal).
    Creature(i8),
}

/// A body's mass (kg), the height of its centre of mass and half its stance (m), at scale 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Figure {
    pub mass: f32,
    pub height: f32,
    pub stance: f32,
}

impl Figure {
    pub const fn new(mass: f32, height: f32, stance: f32) -> Figure {
        Figure {
            mass,
            height,
            stance,
        }
    }
}

pub fn figures(body: Body) -> Figure {
    let t = tuning::get();
    match body {
        Body::Human => Figure::new(t.human_mass, t.human_height, t.human_stance),
        Body::SuperMutant => Figure::new(t.mutant_mass, t.mutant_height, t.mutant_stance),
        Body::Creature(kind) => CREATURE
            .get(usize::try_from(kind).unwrap_or(0))
            .copied()
            .unwrap_or(CREATURE[0]),
    }
}

/// An actor's scale held to 0.25 to 4 (1 for none).
pub fn sane_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(0.25, 4.0)
    } else {
        1.0
    }
}

/// A body's mass by its kind and size: the figure's times `scale` cubed.
pub fn body_mass(body: Body, scale: f32) -> f32 {
    figures(body).mass * sane_scale(scale).powi(3)
}

/// The speed (m/s) a push must give a standing body to topple it: its centre of mass lifted over
/// the stance's edge, `sqrt(2 g (sqrt(h^2 + b^2) - h))`, times [`RECOVERY`] (it steps to catch
/// itself below that), lengths times `scale`, times `threshold` percent. A human: about 1.4 m/s.
pub fn topple_speed(body: Body, scale: f32, threshold: i32) -> f32 {
    let f = figures(body);
    let s = sane_scale(scale);
    let (h, b) = (f.height * s, f.stance * s);
    let rise = (h * h + b * b).sqrt() - h;
    tuning::get().recovery * (2.0 * GRAVITY * rise).sqrt() * threshold.max(0) as f32 / 100.0
}

/// What pushed the body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Push {
    /// A round ([`Round`]).
    Round(Round),
    /// A swing: the weapon's weight (lb, 0 for fists), its reach, the attacker's Strength (1-10)
    /// and skill (0-100), a power attack.
    Swing {
        weight: f32,
        reach: f32,
        strength: f32,
        skill: f32,
        power: bool,
    },
    /// A blast: the game's damage to the body (its falloff with distance included).
    Blast { damage: f32 },
    /// Something of no known calibre (a thrown weapon, an unknown gun): its damage.
    Damage { damage: f32 },
}

/// Armour's share of a push left: `1 / (1 + DT / ARMOUR_DT)`.
pub fn armour_left(dt: f32) -> f32 {
    1.0 / (1.0 + dt.max(0.0) / tuning::get().armour_dt)
}

/// The momentum (N s) `push` puts into a body behind `dt` damage threshold (module documentation).
pub fn transferred(push: Push, dt: f32) -> f32 {
    let t = tuning::get();
    let p = match push {
        Push::Round(r) => r.momentum() * r.share * t.bullet_feel * armour_left(dt * r.threshold),
        Push::Swing {
            weight,
            reach,
            strength,
            skill,
            power,
        } => {
            let mass = weight.max(0.0) * POUND + t.arm_mass;
            let speed = t.swing_speed
                * strength_factor(strength)
                * skill_factor(skill)
                * reach.clamp(0.5, 2.0)
                * if power { t.power_attack } else { 1.0 };
            mass * speed * t.melee_share * armour_left(dt)
        }
        Push::Blast { damage } => damage.max(0.0) * t.blast_per_point,
        Push::Damage { damage } => {
            damage.max(0.0) * t.damage_momentum * t.share_through * t.bullet_feel * armour_left(dt)
        }
    };
    if p.is_finite() {
        p.max(0.0)
    } else {
        0.0
    }
}

/// What a body does when a push gives it `speed` (m/s) and `topple` topples it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    None,
    Stagger,
    Knockdown,
}

pub fn reaction(speed: f32, topple: f32) -> Reaction {
    if speed.is_nan() || speed <= 0.0 {
        Reaction::None
    } else if speed >= topple {
        Reaction::Knockdown
    } else if speed >= topple * tuning::get().stagger_share {
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

/// What the host's stack answers ([`add_hit`]): the momentum left of the earlier hits, the total with this one, the
/// direction of the total (unit), the hits in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stacked {
    pub before: f32,
    pub total: f32,
    pub direction: [f32; 3],
    pub hits: u32,
}

/// Momentum `p` decayed over `seconds` with the half-life `half_life` (seconds).
pub fn decayed(p: f32, seconds: f32, half_life: f32) -> f32 {
    if half_life <= 0.0 || seconds.is_nan() {
        return 0.0;
    }
    p * 0.5f32.powf(seconds.max(0.0) / half_life)
}

/// A stack's momentum and direction after a hit of `p` along `direction` (unit), the earlier
/// `total` and `vector` decayed over `seconds`; the hits counted.
pub fn add_hit(
    total: f32,
    vector: [f32; 3],
    hits: u32,
    seconds: f32,
    half_life: f32,
    p: f32,
    direction: [f32; 3],
) -> (Stacked, [f32; 3]) {
    let k = decayed(1.0, seconds, half_life);
    let before = total * k;
    // A stack faded below a hundredth of its momentum counts no hits any more.
    let hits = if k < 0.01 || before <= 0.0 {
        1
    } else {
        hits + 1
    };
    let v = [0, 1, 2].map(|i| vector[i] * k + direction[i] * p);
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let dir = if l.is_finite() && l > 1e-6 {
        v.map(|x| x / l)
    } else {
        direction
    };
    (
        Stacked {
            before,
            total: before + p,
            direction: dir,
            hits,
        },
        v,
    )
}

/// The launch (game units a second) for a body's speed `speed` (m/s), at most [`MAX_LAUNCH`].
pub fn launch_speed(speed: f32) -> f32 {
    if speed.is_nan() {
        0.0
    } else {
        speed.clamp(0.0, tuning::get().max_launch) * UNITS_PER_METRE
    }
}

/// The launch's direction: the hit's, lifted by [`LIFT`] (the tuning's), of unit length, rising
/// at most [`LIFT`] over the hit's own rise (none for a hit going down) before the length is
/// made one, and at most [`MOST_RISE`] after. Patrick's 08-21-49 clip: an anti-materiel hit at
/// (-0.16, -0.93, 0.33) launched along (-0.11, -0.67, 0.73), an effective lift of about 0.68 on a
/// hit already rising: he flew up a metre and landed on his feet.
pub fn launch_direction(direction: [f32; 3]) -> [f32; 3] {
    let z = direction[2] + tuning::get().lift;
    let z = if z.is_finite() {
        z.min(direction[2].max(0.0) + LIFT)
    } else {
        z
    };
    let d = [direction[0], direction[1], z];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if l.is_nan() || l < 1e-3 {
        return [0.0, 0.0, 1.0];
    }
    let d = d.map(|x| x / l);
    if d[2] <= MOST_RISE {
        return d;
    }
    // Held to the most rise, the rest along the ground (straight up: none left along it).
    let flat = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if flat < 1e-6 {
        return [0.0, 0.0, 1.0];
    }
    let k = (1.0 - MOST_RISE * MOST_RISE).sqrt() / flat;
    [d[0] * k, d[1] * k, MOST_RISE]
}

/// The launch's direction rises at most this much (its unit vector's z, [`launch_direction`]).
pub const MOST_RISE: f32 = 0.45;

#[cfg(test)]
mod tests {
    /// Lying: the pelvis low and the torso near level; a kneeling slump (the pelvis 23 units up,
    /// the torso upright) or one sitting (the torso level, the pelvis high) does not lie.
    #[test]
    fn only_a_body_on_the_ground_lies() {
        assert!(lying(8.0, [0.0, 0.0, 10.0], [0.0, 40.0, 14.0], 1.0));
        assert!(
            !lying(23.0, [0.0, 0.0, 40.0], [0.0, 5.0, 75.0], 1.0),
            "kneeling"
        );
        assert!(
            !lying(8.0, [0.0, 0.0, 10.0], [0.0, 20.0, 45.0], 1.0),
            "propped up"
        );
        assert!(
            !lying(20.0, [0.0, 0.0, 22.0], [0.0, 40.0, 24.0], 1.0),
            "on something"
        );
        let l = lean([0.0, 0.0, 40.0], [3.0, 4.0, 75.0], [0.0, 1.0, 0.0]);
        assert!((l[0] - 0.6).abs() < 1e-5 && (l[1] - 0.8).abs() < 1e-5);
    }

    /// The launch rises at most LIFT over a rising hit, and at most 0.45 whatever the tuning;
    /// a level hit is lifted by LIFT.
    #[test]
    fn the_launch_rises_little() {
        let d = launch_direction([-0.16, -0.93, 0.33]);
        assert!(d[2] <= MOST_RISE + 1e-5, "{d:?}");
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        assert!((l - 1.0).abs() < 1e-4);
        let level = launch_direction([0.0, -1.0, 0.0]);
        assert!(
            (level[2] - LIFT / (1.0 + LIFT * LIFT).sqrt()).abs() < 1e-4,
            "{level:?}"
        );
        let down = launch_direction([0.0, -0.9, -0.43]);
        assert!(down[2] <= LIFT, "{down:?}");
    }

    use super::*;

    /// Bodies by mass and stance: a human topples at about 1.4 m/s; a deathclaw, a giant and a
    /// bigger one take more; the threshold scales it; mass grows with size cubed.
    #[test]
    fn bodies_topple_by_stance_and_weigh_by_size() {
        let human = topple_speed(Body::Human, 1.0, 100);
        assert!((1.3..1.5).contains(&human), "{human}");
        assert!(topple_speed(Body::SuperMutant, 1.0, 100) > human);
        assert!(
            topple_speed(Body::Creature(3), 1.0, 100) > 2.0 * human,
            "deathclaw"
        );
        assert!(
            topple_speed(Body::Creature(7), 1.0, 100) > topple_speed(Body::Creature(3), 1.0, 100)
        );
        assert!(
            topple_speed(Body::Creature(0), 1.5, 100) > topple_speed(Body::Creature(0), 1.0, 100)
        );
        assert!((topple_speed(Body::Human, 1.0, 200) - 2.0 * human).abs() < 1e-4);
        assert_eq!(body_mass(Body::Human, 1.0), 80.0);
        assert!((body_mass(Body::Human, 2.0) - 640.0).abs() < 1e-3);
        assert_eq!(figures(Body::Creature(-1)), CREATURE[0]);
    }

    /// Momentum transfer: an anti-materiel round knocks an unarmoured human down and launches a
    /// dog farther; a 5.56mm round only staggers at most; a deathclaw stands; pellets add up; a
    /// sledgehammer's power attack topples, a knife does not; a grenade's close blast does.
    #[test]
    fn hits_push_by_momentum_over_mass() {
        use crate::rounds::Round;
        let round = |g: f32, v: f32, share: f32, pellet| Round {
            mass: g / 1000.0,
            speed: v,
            share,
            threshold: 1.0,
            pellet,
        };
        let human = topple_speed(Body::Human, 1.0, 100);
        let speed = |push: Push, body: Body| transferred(push, 0.0) / body_mass(body, 1.0);
        let fifty = Push::Round(round(42.0, 900.0, 0.6, false));
        assert_eq!(
            reaction(speed(fifty, Body::Human), human),
            Reaction::Knockdown
        );
        assert!(
            speed(fifty, Body::Creature(0)) > 2.0 * speed(fifty, Body::Human),
            "a dog flies"
        );
        let claw = topple_speed(Body::Creature(3), 1.0, 100);
        assert_eq!(
            reaction(speed(fifty, Body::Creature(3)), claw),
            Reaction::None
        );
        let rifle = Push::Round(round(4.0, 990.0, 0.6, false));
        assert_ne!(
            reaction(speed(rifle, Body::Human), human),
            Reaction::Knockdown
        );
        let pellet = transferred(Push::Round(round(3.5, 400.0, 1.0, true)), 0.0);
        assert!(
            8.0 * pellet / 80.0 > human,
            "a close shotgun blast, all pellets"
        );
        assert!(pellet / 80.0 < human * STAGGER_SHARE, "one pellet");
        let swing = |weight: f32, power| Push::Swing {
            weight,
            reach: 1.0,
            strength: 5.0,
            skill: 50.0,
            power,
        };
        assert_eq!(
            reaction(speed(swing(20.0, true), Body::Human), human),
            Reaction::Knockdown
        );
        assert_eq!(
            reaction(speed(swing(1.0, false), Body::Human), human),
            Reaction::None
        );
        assert_eq!(
            reaction(speed(Push::Blast { damage: 100.0 }, Body::Human), human),
            Reaction::Knockdown
        );
        // Armour takes its share of a round, none of a blast's (the game took it already).
        assert!(transferred(fifty, 20.0) < transferred(fifty, 0.0));
        assert_eq!(armour_left(0.0), 1.0);
        assert_eq!(reaction(f32::NAN, human), Reaction::None);
    }

    /// The stack: hits add up, fade by half each half-life, point the momentum-weighted way; a
    /// stack faded to nothing counts the next hit alone.
    #[test]
    fn impacts_stack_and_fade() {
        let east = [1.0, 0.0, 0.0];
        let north = [0.0, 1.0, 0.0];
        let (a, v) = add_hit(0.0, [0.0; 3], 0, 0.0, 0.3, 20.0, east);
        assert_eq!((a.before, a.total, a.hits), (0.0, 20.0, 1));
        // Eight landed pellets at once: eight times one.
        let (mut s, mut v) = (a, v);
        for _ in 1..8 {
            let (n, nv) = add_hit(s.total, v, s.hits, 0.0, 0.3, 20.0, east);
            s = n;
            v = nv;
        }
        assert_eq!((s.total, s.hits), (160.0, 8));
        // One half-life later half is left.
        let (b, _) = add_hit(20.0, [20.0, 0.0, 0.0], 1, 0.3, 0.3, 10.0, north);
        assert!((b.before - 10.0).abs() < 1e-3 && (b.total - 20.0).abs() < 1e-3);
        assert_eq!(b.hits, 2);
        // The direction is the momentum-weighted sum: equal parts east and north.
        assert!((b.direction[0] - b.direction[1]).abs() < 1e-4 && b.direction[0] > 0.7);
        // Faded out: the hit alone.
        let (c, _) = add_hit(20.0, [20.0, 0.0, 0.0], 5, 10.0, 0.3, 10.0, north);
        assert_eq!(c.hits, 1);
        assert!(c.before < 0.01 && c.direction[1] > 0.99);
        assert!((decayed(8.0, 0.6, 0.3) - 2.0).abs() < 1e-4);
        assert_eq!(decayed(8.0, 0.0, 0.0), 0.0);
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
        assert_eq!(launch_speed(4.0), 280.0);
        assert_eq!(launch_speed(100.0), MAX_LAUNCH * UNITS_PER_METRE);
        assert_eq!(launch_speed(f32::NAN), 0.0);
        // Along the hit, a little up, of unit length.
        let d = launch_direction([1.0, 0.0, 0.0]);
        assert!(d[0] > 0.9 && d[2] > 0.2);
        assert!(((d[0] * d[0] + d[2] * d[2]).sqrt() - 1.0).abs() < 1e-5);
        assert_eq!(launch_direction([f32::NAN, 0.0, 0.0]), [0.0, 0.0, 1.0]);
    }
}
