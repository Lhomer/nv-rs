//! What hits leave in a living actor beyond the moment (Patrick, 2026-11-15; ideas after
//! blugart-dev's Kickback, no code of it): kept per actor across reactions, so the host and the
//! lab carry it from hit to hit.
//!
//! - **Fatigue**: each hit tires the muscles; the active body's motors are weaker while it lasts
//!   ([`Condition::motor_scale`]), setting in over a fraction of a second ([`FATIGUE_RISE`]) and
//!   fading slowly ([`FATIGUE_HALF`]).
//! - **Pain**: each hit's severity (its push against the speed that topples the body, times
//!   [`PAIN_GAIN`], at most [`PAIN_PER_HIT`] for the hits of one moment: a shot's pellets) adds up
//!   and fades ([`PAIN_HALF`]); sustained fire escalates deterministically from a flinch to a stagger
//!   ([`PAIN_STAGGER`]) to a fall ([`PAIN_FALL`]) whatever each hit alone would do
//!   ([`Condition::stage`]).
//! - **Injury**: a hit on an arm or a leg hurts that limb ([`Region`]); it heals slowly
//!   ([`INJURY_HALF`]); a crippled limb (the game's limb condition at 0) is fully hurt. A hurt arm's
//!   motors are weak (it dangles); a hurt leg's knee sags and the pelvis drops over it, and it
//!   trips over sooner ([`Condition::leg_weakness`], [`Condition::sag`]).
//! - **Counter-side bracing** is the body's own (`crate::active::Body::hit`): bones away from a
//!   hit stiffen, the hit side softens.

#[allow(unused_imports)]
use crate::fm::Fm;
/// A body's region: the four limbs (left, right), the torso and the head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
    Torso,
    Head,
}

impl Region {
    /// The limb's slot in [`Condition::injury`], none for the torso and the head.
    pub fn limb(self) -> Option<usize> {
        match self {
            Region::LeftArm => Some(0),
            Region::RightArm => Some(1),
            Region::LeftLeg => Some(2),
            Region::RightLeg => Some(3),
            Region::Torso | Region::Head => None,
        }
    }
}

/// Half-lives (s): fatigue, pain, a limb's injury.
pub const FATIGUE_HALF: f32 = 8.0;
/// How long fatigue takes to set in (s, a time constant).
pub const FATIGUE_RISE: f32 = 1.0;
pub const PAIN_HALF: f32 = 2.5;
pub const INJURY_HALF: f32 = 45.0;
/// How much of a hit's severity (its push over the topple speed) becomes fatigue and a limb's
/// injury; the most fatigue takes off the motors.
pub const FATIGUE_GAIN: f32 = 0.35;
pub const INJURY_GAIN: f32 = 0.8;
pub const FATIGUE_MOST: f32 = 0.5;
/// Pain (summed severities) at which a hit staggers at least, and knocks down.
pub const PAIN_STAGGER: f32 = 1.2;
pub const PAIN_FALL: f32 = 2.6;
/// Pain a hit adds per unit of its severity, and at most (a single hit, however hard, is one
/// hit's pain: only sustained fire adds up to a fall); hits closer together than
/// `PAIN_TOGETHER` (s) are one hit to it.
pub const PAIN_GAIN: f32 = 1.5;
pub const PAIN_PER_HIT: f32 = 0.6;
pub const PAIN_TOGETHER: f32 = 0.05;
/// A hurt leg's knee sag (rad) and pelvis drop (m) when fully hurt; a hurt arm's motor weakness
/// (a dip) when fully hurt.
pub const KNEE_SAG: f32 = 0.35;
pub const PELVIS_SAG: f32 = 0.06;
pub const ARM_DANGLE: f32 = 0.8;

/// What pain alone escalates a hit to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Flinch,
    Stagger,
    Fall,
}

/// What hits have left in an actor (module documentation).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Condition {
    /// Seconds since the last hit, and the pain the hits of this moment have added (the pellets
    /// of one shot, a burst's rounds within [`PAIN_TOGETHER`]: one hit to pain).
    pub since: f32,
    pub together: f32,
    pub fatigue: f32,
    /// Fatigue as the muscles feel it: it sets in over [`FATIGUE_RISE`] (a motor never loses its
    /// strength from one moment to the next).
    pub felt: f32,
    pub pain: f32,
    /// Left arm, right arm, left leg, right leg: 0 (whole) to 1 (crippled).
    pub injury: [f32; 4],
}

fn fade(x: f32, dt: f32, half: f32) -> f32 {
    x * 0.5f32.dpowf(dt.max(0.0) / half.max(1e-3))
}

impl Condition {
    /// `dt` seconds of rest.
    pub fn decay(&mut self, dt: f32) {
        self.since += dt.max(0.0);
        self.fatigue = fade(self.fatigue, dt, FATIGUE_HALF);
        let k = 1.0 - (-dt.max(0.0) / FATIGUE_RISE).dexp();
        self.felt += (self.fatigue - self.felt) * k;
        self.pain = fade(self.pain, dt, PAIN_HALF);
        for i in &mut self.injury {
            *i = fade(*i, dt, INJURY_HALF);
        }
    }

    /// A hit of `severity` (its push over the speed that topples the body) on `region`.
    pub fn hit(&mut self, severity: f32, region: Region) {
        let s = if severity.is_finite() {
            severity.clamp(0.0, 4.0)
        } else {
            0.0
        };
        self.fatigue = (self.fatigue + s * FATIGUE_GAIN).min(1.0);
        // Pain: this hit's share, held to one hit's for the hits of this moment.
        let share = s * PAIN_GAIN;
        let before = if self.since < PAIN_TOGETHER {
            self.together
        } else {
            0.0
        };
        let now = (before + share).min(PAIN_PER_HIT);
        self.pain += (now - before).max(0.0);
        self.together = now;
        self.since = 0.0;
        if let Some(l) = region.limb() {
            self.injury[l] = (self.injury[l] + s * INJURY_GAIN).min(1.0);
        }
    }

    /// A limb crippled (the game's limb condition at 0): fully hurt until it heals.
    pub fn cripple(&mut self, region: Region) {
        if let Some(l) = region.limb() {
            self.injury[l] = 1.0;
        }
    }

    /// The motors' strength left by fatigue (1 rested, `1 - FATIGUE_MOST` worn out).
    pub fn motor_scale(&self) -> f32 {
        1.0 - FATIGUE_MOST * self.felt.clamp(0.0, 1.0)
    }

    /// What pain alone escalates the next hit to.
    pub fn stage(&self) -> Stage {
        if self.pain >= PAIN_FALL {
            Stage::Fall
        } else if self.pain >= PAIN_STAGGER {
            Stage::Stagger
        } else {
            Stage::Flinch
        }
    }

    /// The more hurt leg's injury (0 to 1): a stumble pitches over sooner.
    pub fn leg_weakness(&self) -> f32 {
        self.injury[2].max(self.injury[3])
    }

    /// A leg's sag (`side` 0 left, 1 right): the knee's extra bend (rad) and the pelvis's drop (m).
    pub fn sag(&self, side: usize) -> (f32, f32) {
        let i = self.injury[2 + side.min(1)].clamp(0.0, 1.0);
        (KNEE_SAG * i, PELVIS_SAG * i)
    }

    /// An arm's extra motor weakness (`side` 0 left, 1 right; a dip): it dangles.
    pub fn dangle(&self, side: usize) -> f32 {
        ARM_DANGLE * self.injury[side.min(1)].clamp(0.0, 1.0)
    }
}

/// A hit's reaction `decision` (`crate::knock::reaction`) escalated by pain's `stage`: never less
/// than pain makes it (sustained fire staggers, then knocks down, deterministically).
pub fn escalate(decision: crate::knock::Reaction, stage: Stage) -> crate::knock::Reaction {
    use crate::knock::Reaction;
    match (decision, stage) {
        (_, Stage::Fall) => Reaction::Knockdown,
        (Reaction::None, Stage::Stagger) => Reaction::Stagger,
        (d, _) => d,
    }
}

/// The region a bone of `role` on `side` (0 left, 1 right, 2 the middle) belongs to.
pub fn region_of(role: crate::flinch::Role, side: u8) -> Region {
    use crate::flinch::Role;
    match (role, side) {
        (Role::Clavicle | Role::UpperArm | Role::Forearm | Role::Hand, 0) => Region::LeftArm,
        (Role::Clavicle | Role::UpperArm | Role::Forearm | Role::Hand, _) => Region::RightArm,
        (Role::Thigh | Role::Calf | Role::Foot, 0) => Region::LeftLeg,
        (Role::Thigh | Role::Calf | Role::Foot, _) => Region::RightLeg,
        (Role::Neck | Role::Head, _) => Region::Head,
        _ => Region::Torso,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sustained fire escalates in order: light hits close together stagger, then fell; apart they
    /// do not add up.
    #[test]
    fn sustained_fire_escalates() {
        let mut c = Condition::default();
        let mut stages = Vec::new();
        for _ in 0..14 {
            c.hit(0.17, Region::Torso);
            stages.push(c.stage());
            c.decay(0.1);
        }
        assert_eq!(stages[0], Stage::Flinch);
        assert!(stages.windows(2).all(|w| w[0] <= w[1]), "{stages:?}");
        assert_eq!(*stages.last().unwrap(), Stage::Fall);
        let mut apart = Condition::default();
        for _ in 0..14 {
            apart.hit(0.17, Region::Torso);
            apart.decay(5.0);
        }
        assert_eq!(apart.stage(), Stage::Flinch);
        // One hard hit, or one shotgun shot's pellets together, is one hit's pain.
        let mut one = Condition::default();
        one.hit(3.0, Region::Torso);
        assert!(one.pain <= PAIN_PER_HIT + 1e-6);
        let mut pellets = Condition::default();
        for _ in 0..9 {
            pellets.hit(0.2, Region::Torso);
            pellets.decay(0.001);
        }
        assert!(pellets.pain <= PAIN_PER_HIT + 1e-6, "{}", pellets.pain);
    }

    /// Fatigue weakens the motors and fades slowly; a shot leg sags and heals slowly; a crippled
    /// arm dangles fully.
    #[test]
    fn fatigue_and_injury_last() {
        let mut c = Condition::default();
        for _ in 0..4 {
            c.hit(1.0, Region::LeftLeg);
        }
        assert_eq!(c.motor_scale(), 1.0, "not at once");
        c.decay(2.0);
        assert!(c.motor_scale() < 0.65, "{}", c.motor_scale());
        assert!(c.sag(0).0 > 0.3 && c.sag(1).0 == 0.0);
        c.decay(10.0);
        assert!(
            c.motor_scale() > 0.65 && c.motor_scale() < 0.9,
            "{}",
            c.motor_scale()
        );
        assert!(c.sag(0).0 > 0.2, "an injury lasts longer than fatigue");
        c.cripple(Region::RightArm);
        assert_eq!(c.dangle(1), ARM_DANGLE);
        c.decay(600.0);
        assert!(c.leg_weakness() < 0.01 && c.motor_scale() > 0.99);
    }
}
