//! The ballistic rounds' pure parts (Patrick, 2026-11-14): the calibres, the ammo loads, the
//! penetration costs and the share of a round's momentum left in a body. The host's
//! `crate::ballistics` re-exports this module and adds the game's side (the projectile forms, the
//! shots, the penetration); its documentation explains them.

use crate::tuning;

/// A calibre: muzzle velocity (m/s), bullet mass (g), drop factor (the form's gravity: the drag the
/// game does not model, more for slow, light-for-calibre rounds), penetration power and range (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caliber {
    pub name: &'static str,
    pub velocity: f32,
    pub mass: f32,
    pub drop: f32,
    pub penetration: f32,
    pub range: f32,
}

const fn cal(
    name: &'static str,
    velocity: f32,
    mass: f32,
    drop: f32,
    penetration: f32,
    range: f32,
) -> Caliber {
    Caliber {
        name,
        velocity,
        mass,
        drop,
        penetration,
        range,
    }
}

/// The calibres, by the words of the ammo's name that pick them (the first match wins, so the
/// longer names come first).
pub const CALIBERS: [(&[&str], Caliber); 21] = [
    (
        &[".45-70", "45-70"],
        cal(".45-70 Gov't", 480.0, 26.0, 1.4, 45.0, 400.0),
    ),
    (
        &[".50 mg", ".50 bmg", ".50 cal", "50mg", ".50"],
        cal(".50 MG", 900.0, 42.0, 0.9, 120.0, 1800.0),
    ),
    (
        &["12.7mm", "12.7"],
        cal("12.7mm", 450.0, 20.0, 1.3, 30.0, 300.0),
    ),
    (
        &[".308", "7.62"],
        cal(".308", 850.0, 11.3, 1.0, 60.0, 1200.0),
    ),
    (&["5.56"], cal("5.56mm", 990.0, 4.0, 1.0, 40.0, 800.0)),
    (
        &["4.73"],
        cal("4.73mm caseless", 930.0, 3.3, 1.0, 35.0, 700.0),
    ),
    (&["5mm"], cal("5mm", 900.0, 2.6, 1.1, 30.0, 600.0)),
    (
        &["2mm ec", "2mm"],
        cal("2mm EC", 1800.0, 2.0, 0.5, 150.0, 2000.0),
    ),
    (&[".357"], cal(".357 Magnum", 440.0, 10.2, 1.4, 25.0, 250.0)),
    (&[".44"], cal(".44 Magnum", 450.0, 15.6, 1.4, 28.0, 250.0)),
    (
        &[".45 auto", ".45acp", ".45"],
        cal(".45 Auto", 260.0, 14.9, 1.7, 18.0, 150.0),
    ),
    (&["10mm"], cal("10mm", 400.0, 11.7, 1.5, 22.0, 200.0)),
    (&["9mm"], cal("9mm", 360.0, 8.0, 1.6, 18.0, 150.0)),
    (&[".32"], cal(".32", 280.0, 5.0, 1.8, 10.0, 100.0)),
    (&[".22"], cal(".22LR", 330.0, 2.6, 1.9, 8.0, 120.0)),
    (
        &["12 ga", "12 gauge", "12ga", "shotgun shell"],
        cal("12 gauge", 400.0, 3.5, 2.0, 6.0, 80.0),
    ),
    (
        &["20 ga", "20 gauge", "20ga"],
        cal("20 gauge", 400.0, 2.6, 2.0, 5.0, 70.0),
    ),
    (
        &["railway spike"],
        cal("railway spike", 300.0, 150.0, 1.5, 40.0, 150.0),
    ),
    (&["bb"], cal("BB", 100.0, 0.35, 2.5, 1.0, 40.0)),
    (
        &["harpoon", "spear"],
        cal("harpoon", 120.0, 300.0, 2.0, 20.0, 60.0),
    ),
    (
        &["round", "cartridge", "bullet"],
        cal("unknown round", 600.0, 8.0, 1.2, 25.0, 400.0),
    ),
];

/// An ammo name's calibre, `None` for ammo that is no bullet (energy cells, fuel, grenades).
pub fn caliber_of(name: &str) -> Option<Caliber> {
    let n = name.to_ascii_lowercase();
    let energy = [
        "cell",
        "charge",
        "fuel",
        "rocket",
        "grenade",
        "missile",
        "mini nuke",
        "battery",
        "flare",
    ];
    if energy.iter().any(|w| n.contains(w)) {
        return None;
    }
    CALIBERS
        .iter()
        .find(|(words, _)| words.iter().any(|w| n.contains(w)))
        .map(|&(_, c)| c)
}

/// An ammo subtype.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    Standard,
    HollowPoint,
    ArmorPiercing,
    Jsp,
    Surplus,
    HandLoad,
    Match,
    Incendiary,
    Explosive,
    Slug,
    Magnum,
}

/// What a load changes: velocity, mass, penetration and transfer factors, and how much the damage
/// threshold counts against its momentum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoadMods {
    pub velocity: f32,
    pub mass: f32,
    pub penetration: f32,
    pub transfer: f32,
    pub threshold: f32,
}

impl Load {
    /// Whether the round goes on through a body (an expanding or bursting one stops in it).
    pub fn over_penetrates(self) -> bool {
        !matches!(self, Load::HollowPoint | Load::Explosive)
    }

    pub fn mods(self) -> LoadMods {
        let m = |velocity, mass, penetration, transfer, threshold| LoadMods {
            velocity,
            mass,
            penetration,
            transfer,
            threshold,
        };
        match self {
            Load::Standard => m(1.0, 1.0, 1.0, 1.0, 1.0),
            // Expands: stops in the body, gives it most of its momentum, armour stops it well.
            Load::HollowPoint => m(0.97, 1.0, 0.25, 1.4, 1.6),
            // Hard core: through armour, cover and bodies, little of its push stays in the target.
            Load::ArmorPiercing => m(1.03, 0.95, 2.5, 0.7, 0.4),
            Load::Jsp => m(1.0, 1.0, 0.8, 1.2, 1.1),
            Load::Surplus => m(0.9, 1.0, 0.9, 0.9, 1.1),
            Load::HandLoad => m(1.1, 1.0, 1.1, 1.05, 1.0),
            Load::Match => m(1.08, 1.0, 1.05, 1.0, 1.0),
            Load::Incendiary => m(1.0, 0.95, 0.8, 1.0, 1.0),
            Load::Explosive => m(0.95, 1.05, 0.3, 1.3, 1.2),
            // One heavy ball for the shotgun: eight times a pellet's mass, penetrates.
            Load::Slug => m(1.1, 8.0, 4.0, 1.0, 0.9),
            Load::Magnum => m(1.1, 1.2, 1.1, 1.1, 1.0),
        }
    }
}

/// The ammo's load: by the words of its name; else, from its effects, `dt_mult` (the damage
/// threshold's multiplier, 1 for none) and `dt_minus` (its subtraction, 0 for none).
pub fn load_of(name: &str, dt_mult: f32, dt_minus: f32) -> Load {
    let n = name.to_ascii_lowercase();
    let has = |w: &[&str]| w.iter().any(|x| n.contains(x));
    if has(&["hollow point", " hp", "(hp)", "hp)", "jhp"]) {
        Load::HollowPoint
    } else if has(&["armor piercing", "armour piercing", " ap", "(ap)", "ap)"]) {
        Load::ArmorPiercing
    } else if has(&["jsp", "soft point"]) {
        Load::Jsp
    } else if has(&["surplus"]) {
        Load::Surplus
    } else if has(&["hand load", "handload", "hand-load"]) {
        Load::HandLoad
    } else if has(&["match"]) {
        Load::Match
    } else if has(&["incendiary", "dragon"]) {
        Load::Incendiary
    } else if has(&["explosive"]) {
        Load::Explosive
    } else if has(&["slug"]) {
        Load::Slug
    } else if has(&["magnum"]) && (n.contains("ga") || n.contains("gauge") || n.contains("shell")) {
        Load::Magnum
    } else if dt_mult > 1.5 {
        Load::HollowPoint
    } else if dt_minus > 0.0 || dt_mult < 0.75 {
        Load::ArmorPiercing
    } else {
        Load::Standard
    }
}

/// Damage points of the hit force per newton-second of momentum (a 5.56mm round, 4 N s: 24).
pub const POINTS_PER_NEWTON_SECOND: f32 = 6.0;
/// The share of the muzzle velocity the form's speed is set to (the average over a usual range:
/// the drag the game does not model).
pub const AVERAGE_SPEED: f32 = 0.9;
/// The fastest a form is made, in units a second (about 1,700 m/s).
pub const MAX_FORM_SPEED: f32 = 120_000.0;
/// What a body costs a round (penetration power), and the most passes and continuations.
pub const BODY_COST: f32 = 15.0;
pub const MAX_PASSES: u8 = 2;
pub const MAX_CONTINUATIONS: u32 = 24;
/// The share of power a round needs left to go on.
pub const MIN_LEFT: f32 = 0.2;
/// A body's depth the continuation starts past, in units.
pub const BODY_DEPTH: f32 = 45.0;

/// A penetrable material (the game's `eMaterial`): its usual thickness in a level (cm) and what a
/// centimetre of it costs a round; `None` for one no round passes (stone, concrete, earth, heavy
/// metal, water).
pub fn material(material: u32) -> Option<(f32, f32)> {
    Some(match material {
        1 => (0.5, 1.0),            // cloth
        3 => (0.6, 3.0),            // glass
        6 => (10.0, 1.0),           // organic
        9 => (4.0, 1.5),            // wood: a plank, a door
        12 => (10.0, 1.5),          // heavy wood: a beam, a table top
        13 | 28 => (0.3, 3.0),      // chain (fences), a shopping cart
        16 => (0.4, 40.0),          // hollow metal: two walls of a locker, a box
        17 => (0.15, 40.0),         // sheet metal
        20 | 22 => (0.4, 40.0),     // a car's body, a hollow part
        23 => (0.3, 40.0),          // a barrel's wall
        24 | 25 | 29 => (0.3, 1.5), // a bottle, a soda can, a lunchbox
        _ => return None,
    })
}

/// What passing a thin object of `material` costs a round, `None` for one it cannot pass.
pub fn material_cost(material_id: u32) -> Option<f32> {
    material(material_id).map(|(thickness, per)| thickness * per)
}

/// The power left after a pass costing `cost` of a round of power `power` (the share of its
/// speed it keeps is the square root of the share of power left), `None` when it stops.
pub fn after_pass(power: f32, cost: f32) -> Option<f32> {
    let left = power - cost;
    (power > 0.0 && left >= power * MIN_LEFT).then_some(left)
}

/// Momentum points (the hit force's damage points) of `mass` grams at `speed` m/s.
pub fn momentum_points(mass: f32, speed: f32) -> f32 {
    mass / 1000.0 * speed * POINTS_PER_NEWTON_SECOND
}

/// A round's momentum as it reaches the target: its mass (kg), its speed there (m/s), the share of
/// its momentum it leaves in the body ([`transfer_share`]), the factor the damage threshold counts
/// by (its load's), and whether it is one of several pellets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Round {
    pub mass: f32,
    pub speed: f32,
    pub share: f32,
    pub threshold: f32,
    pub pellet: bool,
}

impl Round {
    /// Its momentum, N s.
    pub fn momentum(&self) -> f32 {
        self.mass * self.speed
    }
}

/// The share of its momentum a round leaves in a body: a pellet or a slug stops in it (all); a
/// rifle or pistol bullet mostly goes on through it ([`SHARE_THROUGH`] for a standard round, times
/// the load's transfer: a hollow point expands and gives more, an armour-piercing round less),
/// never more than all.
pub fn transfer_share(load: Load, pellet: bool) -> f32 {
    if pellet || load == Load::Slug {
        1.0
    } else {
        (tuning::get().share_through * load.mods().transfer).clamp(0.05, 1.0)
    }
}

/// A standard round's share of its momentum left in the body it hits.
pub const SHARE_THROUGH: f32 = 0.6;

#[cfg(test)]
mod tests {
    use super::*;

    /// The calibres by name: the longer names first, energy ammo is no bullet.
    #[test]
    fn calibres_are_found_by_name() {
        assert_eq!(caliber_of("9mm Round").unwrap().name, "9mm");
        assert_eq!(caliber_of("5.56mm Round, AP").unwrap().name, "5.56mm");
        assert_eq!(
            caliber_of(".45-70 Gov't Round").unwrap().name,
            ".45-70 Gov't"
        );
        assert_eq!(caliber_of(".45 Auto Round").unwrap().name, ".45 Auto");
        assert_eq!(caliber_of(".50 MG Round").unwrap().name, ".50 MG");
        assert_eq!(caliber_of("12.7mm Round").unwrap().name, "12.7mm");
        assert_eq!(caliber_of("12 Ga. Shotgun Shell").unwrap().name, "12 gauge");
        assert_eq!(caliber_of("Energy Cell"), None);
        assert_eq!(caliber_of("Microfusion Cell"), None);
        assert_eq!(
            caliber_of("Some Modded Round").unwrap().name,
            "unknown round"
        );
    }

    /// 9mm is slow and drops, 5.56 fast and flat, .50 heavy and long.
    #[test]
    fn calibres_differ_as_their_rounds() {
        let nine = caliber_of("9mm Round").unwrap();
        let five = caliber_of("5.56mm Round").unwrap();
        let fifty = caliber_of(".50 MG Round").unwrap();
        assert!(nine.velocity < five.velocity / 2.0 && nine.drop > five.drop);
        assert!(fifty.mass > 10.0 * five.mass && fifty.range > five.range);
    }

    /// Loads by name and, for modded ammo, by their effects.
    #[test]
    fn loads_by_name_and_effects() {
        assert_eq!(load_of("5.56mm Round, AP", 1.0, 0.0), Load::ArmorPiercing);
        assert_eq!(
            load_of(".357 Magnum Round, JHP", 1.0, 0.0),
            Load::HollowPoint
        );
        assert_eq!(load_of(".357 Magnum Round", 1.0, 0.0), Load::Standard);
        assert_eq!(load_of("9mm Round, HP", 1.0, 0.0), Load::HollowPoint);
        assert_eq!(load_of(".308 Round, JSP", 1.0, 0.0), Load::Jsp);
        assert_eq!(load_of("10mm Round, Surplus", 1.0, 0.0), Load::Surplus);
        assert_eq!(load_of(".44 Round, Hand Load", 1.0, 0.0), Load::HandLoad);
        assert_eq!(load_of("12 Ga. Shotgun Shell, Slug", 1.0, 0.0), Load::Slug);
        assert_eq!(
            load_of("12 Ga. Shotgun Shell, Magnum", 1.0, 0.0),
            Load::Magnum
        );
        assert_eq!(load_of("Modded Round X", 3.0, 0.0), Load::HollowPoint);
        assert_eq!(load_of("Modded Round Y", 1.0, 15.0), Load::ArmorPiercing);
        let hp = Load::HollowPoint.mods();
        let ap = Load::ArmorPiercing.mods();
        assert!(hp.transfer > 1.0 && hp.penetration < 0.5 && hp.threshold > 1.0);
        assert!(ap.transfer < 1.0 && ap.penetration > 2.0 && ap.threshold < 1.0);
    }

    /// Pellets and slugs stay in the body; a hollow point leaves more there than a standard
    /// round, an armour-piercing round less.
    #[test]
    fn rounds_leave_their_share() {
        assert_eq!(transfer_share(Load::Standard, true), 1.0);
        assert_eq!(transfer_share(Load::Slug, false), 1.0);
        let standard = transfer_share(Load::Standard, false);
        assert_eq!(standard, SHARE_THROUGH);
        assert!(transfer_share(Load::HollowPoint, false) > standard);
        assert!(transfer_share(Load::ArmorPiercing, false) < standard);
        let fifty = Round {
            mass: 0.042,
            speed: 900.0,
            share: 0.6,
            threshold: 1.0,
            pellet: false,
        };
        assert!((fifty.momentum() - 37.8).abs() < 1e-3);
    }

    /// A pass costs power; too little left stops the round. A 9mm stops in a body, a 5.56mm
    /// goes through one, a .308 through two; glass and a door cost little, a car's body a lot.
    #[test]
    fn passes_cost_power() {
        assert_eq!(after_pass(60.0, 15.0), Some(45.0));
        let nine = caliber_of("9mm Round").unwrap().penetration;
        let five = caliber_of("5.56mm Round").unwrap().penetration;
        let rifle = caliber_of(".308 Round").unwrap().penetration;
        assert_eq!(after_pass(nine, BODY_COST), None);
        assert!(after_pass(five, BODY_COST).is_some());
        let once = after_pass(rifle, BODY_COST).unwrap();
        assert!(after_pass(once, BODY_COST).is_some());
        let ap = five * Load::ArmorPiercing.mods().penetration;
        assert!(
            after_pass(ap, material_cost(20).unwrap()).is_some(),
            "AP through a car door"
        );
        assert_eq!(after_pass(nine, material_cost(20).unwrap()), None);
        assert!(material_cost(3).unwrap() < 2.0, "glass");
        assert_eq!(material_cost(0), None, "stone stops it");
        assert!(!Load::HollowPoint.over_penetrates() && Load::Jsp.over_penetrates());
    }
}
