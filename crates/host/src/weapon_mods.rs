//! The weapon mods attached to a held weapon, read generically from the game's own data (Patrick,
//! 2026-11-14: Weapon Mods Expanded and any other mod plugin), for the weighty combat row's
//! effective weapon stats ([`crate::gunplay::effective_stats`]) and the ballistic bullets
//! ([`crate::ballistics`]). Nothing here names a plugin or a mod.
//!
//! **Where mods live.** A weapon form has three mod slots: the mod item (`TESObjectIMOD`,
//! `pModObjectOne..Three` at `+0x350`), its action type (`eModActionOne..Three` at `+0x180`) and two
//! values (`+0x18C`, `+0x1AC`). Which slots an item has attached is the bit mask (1, 2, 4) of its
//! first extra data list (`ItemChange::GetModSlots`, `004bd820`). The game's own readers of the
//! action types (the damage formula, `TESObjectWEAP::Fire`, the spread, the weight, the item card)
//! give their meaning ([`Effect`]); a mod plugin adds mods in the same three slots, so its mods
//! are read the same way.
//!
//! **What the new systems take from them:** damage (+), weight (−), a split beam's extra
//! projectiles, spread (−) as steadiness, a scope (the weapon's own flag or a scope mod), a
//! silencer, the projectile speed multiplier (the game applies it to the player's shots only; here
//! to anyone's), and the barrel: a mod item whose name says it is a barrel makes the round faster
//! ([`LONG_BARREL`]) or, a short one, slower ([`SHORT_BARREL`]); a silencer slows it a little
//! ([`SUPPRESSOR`]).

use engine::prelude::*;

/// A mod's action type (the game's `eModAction`, as its readers use it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// 1: damage plus the value (the damage formula, `ItemChange::HasModEffectActive(1)`).
    Damage,
    /// 3: minimum spread and spread less the value (`00524b80`, `00524be0`).
    Spread,
    /// 4: weight less the value (`TESObjectWEAP::GetFormWeight`).
    Weight,
    /// 9: projectile speed times the value (`009bca60`, the player's shots).
    ProjectileSpeed,
    /// 11: a silencer (the impact and fire sounds).
    Silence,
    /// 12: a split beam: the value more projectiles, the second value times the spread.
    SplitBeam,
    /// 14: a scope (`TESObjectWEAP::Fire`'s aiming, the biped's scope model).
    Scope,
    /// Any other (clip, rate of fire, condition, VATS, equip time): not used here.
    Other(u32),
}

impl Effect {
    pub fn from_type(kind: u32) -> Effect {
        match kind {
            1 => Effect::Damage,
            3 => Effect::Spread,
            4 => Effect::Weight,
            9 => Effect::ProjectileSpeed,
            11 => Effect::Silence,
            12 => Effect::SplitBeam,
            14 => Effect::Scope,
            k => Effect::Other(k),
        }
    }
}

/// What a barrel mod's name says it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Barrel {
    None,
    Long,
    Short,
}

/// An attached mod: its action, values and what its name says of the barrel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mod {
    pub effect: Effect,
    pub value: f32,
    pub value_two: f32,
    pub barrel: Barrel,
    /// The mod item (`TESObjectIMOD`), 0 when the slot has none.
    pub object: u32,
}

/// The muzzle velocity a long or heavy barrel gives, a short one, and a silencer.
pub const LONG_BARREL: f32 = 1.06;
pub const SHORT_BARREL: f32 = 0.95;
pub const SUPPRESSOR: f32 = 0.96;
/// The steadiness a spread mod buys at most (sway and kick), and the spread that counts as all of
/// it (degrees).
pub const MAX_STEADY: f32 = 0.4;
pub const FULL_SPREAD_CUT: f32 = 1.0;

/// What a mod item's name says of the barrel (any language-neutral spelling a plugin uses is
/// beyond this; the effect values still apply).
pub fn barrel_of(name: &str) -> Barrel {
    let n = name.to_ascii_lowercase();
    if !n.contains("barrel") {
        return Barrel::None;
    }
    let short = [
        "short", "snub", "sawed", "sawn", "compact", "cut down", "cut-down", "chopped",
    ];
    if short.iter().any(|w| n.contains(w)) {
        Barrel::Short
    } else {
        Barrel::Long
    }
}

/// One mod's velocity factor: its barrel, a silencer, and with `speed_mod` the projectile speed
/// effect (left out for the player's shots, which the game already scaled).
pub fn mod_velocity(m: &Mod, speed_mod: bool) -> f32 {
    let mut f = match m.barrel {
        Barrel::Long => LONG_BARREL,
        Barrel::Short => SHORT_BARREL,
        Barrel::None => 1.0,
    };
    match m.effect {
        Effect::Silence => f *= SUPPRESSOR,
        Effect::ProjectileSpeed if speed_mod && m.value.is_finite() && m.value > 0.0 => {
            f *= m.value.clamp(0.25, 4.0)
        }
        _ => {}
    }
    f
}

/// The velocity factor of the attached mods ([`mod_velocity`] of each).
pub fn velocity_factor(mods: &[Mod], speed_mod: bool) -> f32 {
    mods.iter().map(|m| mod_velocity(m, speed_mod)).product()
}

/// The projectiles a split beam adds.
pub fn extra_projectiles(mods: &[Mod]) -> u32 {
    mods.iter()
        .filter(|m| m.effect == Effect::SplitBeam && m.value.is_finite() && m.value > 0.0)
        .map(|m| m.value.min(20.0) as u32)
        .sum()
}

/// The mod effects on the stats the new systems use: damage, weight, projectiles, scope,
/// silencer and steadiness (`min_spread` the weapon's own minimum spread, degrees).
pub fn apply(
    mut stats: crate::gunplay::WeaponStats,
    mods: &[Mod],
    min_spread: f32,
) -> crate::gunplay::WeaponStats {
    for m in mods {
        let v = if m.value.is_finite() { m.value } else { 0.0 };
        match m.effect {
            Effect::Damage => stats.damage = (stats.damage + v).max(0.0),
            Effect::Weight => stats.weight = (stats.weight - v).max(0.0),
            Effect::Spread if v > 0.0 => {
                let full = min_spread.max(FULL_SPREAD_CUT);
                stats.steady *= 1.0 - MAX_STEADY * (v / full).min(1.0);
            }
            Effect::Silence => stats.silenced = true,
            Effect::Scope => stats.scoped = true,
            _ => {}
        }
    }
    stats.projectiles += extra_projectiles(mods);
    stats
}

// --- The game's words. -------------------------------------------------------------------------

/// `TESObjectREFR::IsActor` (virtual `+0x100`), an actor's process.
const IS_ACTOR: u32 = 0x100;
const ACTOR_PROCESS: u32 = 0x68;
const PROCESS_HELD_ITEM: u32 = 0x148;
const ITEM_FORM: u32 = 8;
const GET_MOD_SLOTS: u32 = 0x004b_d820;
const MOD_ACTION: u32 = 0x180;
const MOD_VALUE: u32 = 0x18c;
const MOD_VALUE_TWO: u32 = 0x1ac;
const MOD_OBJECT: u32 = 0x350;
const WEAP: u8 = 0x28;

/// The actor's held weapon item (an `ItemChange`), 0 for none.
pub fn held_item(e: &mut Engine, actor: u32) -> u32 {
    if actor == 0 || !e.vcall(actor, IS_ACTOR, &args![]).bool() {
        return 0;
    }
    let process = e.mem.u32(actor + ACTOR_PROCESS);
    if process == 0 {
        return 0;
    }
    e.vcall(process, PROCESS_HELD_ITEM, &args![]).u32()
}

/// The mods attached to `item` (an `ItemChange` of a weapon), in slot order.
pub fn attached(e: &mut Engine, item: u32) -> Vec<Mod> {
    if item == 0 {
        return Vec::new();
    }
    let weapon = e.mem.u32(item + ITEM_FORM);
    if weapon == 0 || e.mem.u8(weapon + 4) != WEAP {
        return Vec::new();
    }
    let slots = e.call(GET_MOD_SLOTS, &args![item]).u32() & 7;
    let mut mods = Vec::new();
    for i in 0..3u32 {
        if slots & (1 << i) == 0 {
            continue;
        }
        let object = e.mem.u32(weapon + MOD_OBJECT + 4 * i);
        let name = if object != 0 {
            String::from_utf8_lossy(&jip::commands::the_name(e, object)).into_owned()
        } else {
            String::new()
        };
        mods.push(Mod {
            effect: Effect::from_type(e.mem.u32(weapon + MOD_ACTION + 4 * i)),
            value: e.mem.f32(weapon + MOD_VALUE + 4 * i),
            value_two: e.mem.f32(weapon + MOD_VALUE_TWO + 4 * i),
            barrel: barrel_of(&name),
            object,
        });
    }
    mods
}

/// The mods on `actor`'s held item when that item is `weapon` (a hit's or a shot's weapon form);
/// none otherwise.
pub fn on_weapon(e: &mut Engine, actor: u32, weapon: u32) -> Vec<Mod> {
    let item = held_item(e, actor);
    if item == 0 || weapon == 0 || e.mem.u32(item + ITEM_FORM) != weapon {
        return Vec::new();
    }
    attached(e, item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gunplay::WeaponStats;

    fn m(effect: u32, value: f32, value_two: f32) -> Mod {
        Mod {
            effect: Effect::from_type(effect),
            value,
            value_two,
            barrel: Barrel::None,
            object: 0,
        }
    }

    /// Damage up, weight down, a split beam's projectiles, a scope, a silencer, a steadier aim.
    #[test]
    fn mods_change_the_effective_stats() {
        let base = WeaponStats {
            kind: 3,
            damage: 20.0,
            projectiles: 1,
            weight: 4.0,
            ..WeaponStats::default()
        };
        let mods = [
            m(1, 5.0, 0.0),
            m(4, 1.5, 0.0),
            m(12, 2.0, 1.5),
            m(14, 0.0, 0.0),
            m(11, 0.0, 0.0),
            m(3, 0.5, 0.0),
        ];
        let s = apply(base, &mods, 1.0);
        assert_eq!(s.damage, 25.0);
        assert_eq!(s.weight, 2.5);
        assert_eq!(s.projectiles, 3);
        assert!(s.scoped && s.silenced);
        assert!((s.steady - 0.8).abs() < 1e-6, "{}", s.steady);
        assert_eq!(apply(base, &[], 1.0), base, "no mods, the base stats");
        assert_eq!(apply(base, &[m(4, 9.0, 0.0)], 1.0).weight, 0.0);
    }

    /// Barrels by their names, a silencer slows a round, the speed effect only where asked.
    #[test]
    fn barrels_and_silencers_change_the_velocity() {
        assert_eq!(barrel_of("Long Barrel"), Barrel::Long);
        assert_eq!(barrel_of("Heavy Barrel"), Barrel::Long);
        assert_eq!(barrel_of("Sawed-Off Barrel"), Barrel::Short);
        assert_eq!(barrel_of("Short Barrel (WMX)"), Barrel::Short);
        assert_eq!(barrel_of("Extended Magazine"), Barrel::None);
        let long = Mod {
            barrel: Barrel::Long,
            ..m(1, 4.0, 0.0)
        };
        assert!(velocity_factor(&[long], true) > 1.0);
        assert!(velocity_factor(&[m(11, 0.0, 0.0)], true) < 1.0);
        assert_eq!(velocity_factor(&[m(9, 1.5, 0.0)], true), 1.5);
        assert_eq!(velocity_factor(&[m(9, 1.5, 0.0)], false), 1.0);
        assert_eq!(velocity_factor(&[], true), 1.0);
    }
}
