//! What the lab hits with: guns by calibre and load (`ragdoll_core::rounds`, the game's tables),
//! melee weapons by weight and reach, and blasts by damage and radius. Each becomes the push the
//! game's knockdowns measure (`ragdoll_core::knock::Push`), the way the host's
//! `crate::knockdown::measure_hit` makes it.
//!
//! The guns' muzzle velocity multipliers are those of `crates/host/data/gun_velocity.tsv` for the
//! named vanilla guns (1.0 where the table has none); the pellets are the vanilla shotguns'.

use ragdoll_core::knock::Push;
use ragdoll_core::rounds::{caliber_of, transfer_share, Load, Round};

/// A gun: its calibre (an ammo name `caliber_of` reads), pellets a shot, its muzzle velocity
/// multiplier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gun {
    pub id: &'static str,
    pub name: &'static str,
    pub ammo: &'static str,
    pub pellets: u32,
    pub velocity: f32,
}

pub const GUNS: [Gun; 15] = [
    Gun {
        id: "22",
        name: ".22 silenced pistol",
        ammo: ".22LR round",
        pellets: 1,
        velocity: 1.0,
    },
    Gun {
        id: "9mm",
        name: "9mm pistol",
        ammo: "9mm round",
        pellets: 1,
        velocity: 1.0,
    },
    Gun {
        id: "10mm",
        name: "10mm pistol",
        ammo: "10mm round",
        pellets: 1,
        velocity: 1.0,
    },
    Gun {
        id: "45",
        name: ".45 Auto pistol",
        ammo: ".45 Auto round",
        pellets: 1,
        velocity: 1.0,
    },
    Gun {
        id: "357",
        name: ".357 revolver",
        ammo: ".357 magnum round",
        pellets: 1,
        velocity: 1.0,
    },
    Gun {
        id: "44",
        name: ".44 Magnum revolver",
        ammo: ".44 magnum round",
        pellets: 1,
        velocity: 1.0,
    },
    Gun {
        id: "556",
        name: "Service rifle (5.56mm)",
        ammo: "5.56mm round",
        pellets: 1,
        velocity: 1.0,
    },
    Gun {
        id: "308",
        name: "Hunting rifle (.308)",
        ammo: ".308 round",
        pellets: 1,
        velocity: 1.05,
    },
    Gun {
        id: "4570",
        name: "Brush gun (.45-70)",
        ammo: ".45-70 Gov't round",
        pellets: 1,
        velocity: 1.0,
    },
    Gun {
        id: "127",
        name: "12.7mm SMG",
        ammo: "12.7mm round",
        pellets: 1,
        velocity: 0.95,
    },
    Gun {
        id: "50mg",
        name: "Anti-materiel rifle (.50 MG)",
        ammo: ".50 MG round",
        pellets: 1,
        velocity: 1.15,
    },
    Gun {
        id: "2mm",
        name: "Gauss rifle (2mm EC)",
        ammo: "2mm EC",
        pellets: 1,
        velocity: 1.0,
    },
    Gun {
        id: "12ga",
        name: "Hunting shotgun (12 ga)",
        ammo: "12 ga shotgun shell",
        pellets: 7,
        velocity: 1.0,
    },
    Gun {
        id: "sawedoff",
        name: "Sawed-off shotgun (12 ga)",
        ammo: "12 ga shotgun shell",
        pellets: 7,
        velocity: 0.9,
    },
    Gun {
        id: "20ga",
        name: "Single shotgun (20 ga)",
        ammo: "20 ga shotgun shell",
        pellets: 7,
        velocity: 1.0,
    },
];

/// The loads, by the names the lab shows and takes.
pub const LOADS: [(&str, Load); 10] = [
    ("standard", Load::Standard),
    ("hp", Load::HollowPoint),
    ("ap", Load::ArmorPiercing),
    ("jsp", Load::Jsp),
    ("surplus", Load::Surplus),
    ("handload", Load::HandLoad),
    ("match", Load::Match),
    ("explosive", Load::Explosive),
    ("slug", Load::Slug),
    ("magnum", Load::Magnum),
];

/// A melee weapon: weight (lb, 0 for fists) and reach (the game's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Melee {
    pub id: &'static str,
    pub name: &'static str,
    pub weight: f32,
    pub reach: f32,
}

pub const MELEE: [Melee; 8] = [
    Melee {
        id: "fists",
        name: "Fists",
        weight: 0.0,
        reach: 0.75,
    },
    Melee {
        id: "knife",
        name: "Combat knife",
        weight: 1.0,
        reach: 0.75,
    },
    Melee {
        id: "machete",
        name: "Machete",
        weight: 2.0,
        reach: 0.9,
    },
    Melee {
        id: "bat",
        name: "Baseball bat",
        weight: 3.0,
        reach: 1.1,
    },
    Melee {
        id: "powerfist",
        name: "Power fist",
        weight: 6.0,
        reach: 0.75,
    },
    Melee {
        id: "spear",
        name: "Spear",
        weight: 3.0,
        reach: 1.3,
    },
    Melee {
        id: "sledge",
        name: "Sledgehammer",
        weight: 20.0,
        reach: 1.3,
    },
    Melee {
        id: "supersledge",
        name: "Super sledge",
        weight: 20.0,
        reach: 1.3,
    },
];

/// A blast: its damage at its centre and its radius (units). The damage falls off linearly to the
/// radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blast {
    pub id: &'static str,
    pub name: &'static str,
    pub damage: f32,
    pub radius: f32,
}

pub const BLASTS: [Blast; 4] = [
    Blast {
        id: "frag",
        name: "Frag grenade",
        damage: 100.0,
        radius: 350.0,
    },
    Blast {
        id: "dynamite",
        name: "Dynamite",
        damage: 75.0,
        radius: 300.0,
    },
    Blast {
        id: "missile",
        name: "Missile",
        damage: 150.0,
        radius: 300.0,
    },
    Blast {
        id: "nuke",
        name: "Mini nuke",
        damage: 1000.0,
        radius: 1200.0,
    },
];

/// The weapon in hand.
#[derive(Clone, Debug, PartialEq)]
pub enum Weapon {
    Gun {
        gun: Gun,
        load: Load,
    },
    Melee {
        melee: Melee,
        strength: f32,
        skill: f32,
        power: bool,
    },
    Blast {
        blast: Blast,
    },
}

impl Default for Weapon {
    fn default() -> Self {
        Weapon::Gun {
            gun: GUNS[1],
            load: Load::Standard,
        }
    }
}

pub fn gun(id: &str) -> Option<Gun> {
    GUNS.iter().copied().find(|g| g.id.eq_ignore_ascii_case(id))
}
pub fn melee(id: &str) -> Option<Melee> {
    MELEE
        .iter()
        .copied()
        .find(|m| m.id.eq_ignore_ascii_case(id))
}
pub fn blast(id: &str) -> Option<Blast> {
    BLASTS
        .iter()
        .copied()
        .find(|b| b.id.eq_ignore_ascii_case(id))
}
pub fn load(id: &str) -> Option<Load> {
    LOADS
        .iter()
        .find(|l| l.0.eq_ignore_ascii_case(id))
        .map(|l| l.1)
}
pub fn load_name(load: Load) -> &'static str {
    LOADS
        .iter()
        .find(|l| l.1 == load)
        .map_or("standard", |l| l.0)
}

/// A gun's round as it reaches the body (as the host's `crate::ballistics::weapon_round` makes it:
/// the calibre's mass and muzzle speed, by the load and the gun's multiplier) and its projectiles a
/// shot (a slug is one).
pub fn round(gun: Gun, load: Load) -> Option<(Round, u32)> {
    let caliber = caliber_of(gun.ammo)?;
    let mods = load.mods();
    let pellets = if load == Load::Slug {
        1
    } else {
        gun.pellets.max(1)
    };
    let pellet = pellets > 1;
    Some((
        Round {
            mass: caliber.mass * mods.mass / 1000.0,
            speed: caliber.velocity * mods.velocity * gun.velocity,
            share: transfer_share(load, pellet),
            threshold: mods.threshold,
            pellet,
        },
        pellets,
    ))
}

/// A blast's damage at `distance` units from its centre.
pub fn blast_damage(blast: Blast, distance: f32) -> f32 {
    if blast.radius <= 0.0 {
        return 0.0;
    }
    (blast.damage * (1.0 - distance / blast.radius)).max(0.0)
}

/// The push of a melee weapon.
pub fn swing(melee: Melee, strength: f32, skill: f32, power: bool) -> Push {
    Push::Swing {
        weight: melee.weight,
        reach: melee.reach,
        strength,
        skill,
        power,
    }
}

/// The weapons as JSON for the page's selectors.
pub fn json() -> String {
    let guns: Vec<String> = GUNS
        .iter()
        .map(|g| {
            format!(
                "{{\"id\":\"{}\",\"name\":\"{}\",\"pellets\":{}}}",
                g.id, g.name, g.pellets
            )
        })
        .collect();
    let melee: Vec<String> = MELEE
        .iter()
        .map(|m| format!("{{\"id\":\"{}\",\"name\":\"{}\"}}", m.id, m.name))
        .collect();
    let blasts: Vec<String> = BLASTS
        .iter()
        .map(|b| {
            format!(
                "{{\"id\":\"{}\",\"name\":\"{}\",\"damage\":{}}}",
                b.id, b.name, b.damage
            )
        })
        .collect();
    let loads: Vec<String> = LOADS.iter().map(|l| format!("\"{}\"", l.0)).collect();
    format!(
        "{{\"guns\":[{}],\"melee\":[{}],\"blasts\":[{}],\"loads\":[{}]}}",
        guns.join(","),
        melee.join(","),
        blasts.join(","),
        loads.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_gun_has_its_calibre() {
        for g in GUNS {
            assert!(round(g, Load::Standard).is_some(), "{}", g.name);
        }
        let (fifty, _) = round(gun("50mg").unwrap(), Load::Standard).unwrap();
        let (nine, _) = round(gun("9mm").unwrap(), Load::Standard).unwrap();
        assert!(fifty.momentum() > 5.0 * nine.momentum());
        let (_, pellets) = round(gun("12ga").unwrap(), Load::Slug).unwrap();
        assert_eq!(pellets, 1);
        assert_eq!(blast_damage(BLASTS[0], 0.0), 100.0);
        assert_eq!(blast_damage(BLASTS[0], 1000.0), 0.0);
        assert!(json().contains("\"50mg\""));
    }
}
