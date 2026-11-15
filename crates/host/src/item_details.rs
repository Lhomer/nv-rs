//! Detailed stats of a weapon or an ammo item (Patrick, 2026-11-14), part of the weighty combat
//! row, on the item card that the equipment stats row fills ([`crate::equipment_stats`]): while a
//! gun or an ammo of a known calibre is highlighted in the Pip-Boy's inventory, a container
//! (looting) or barter, the card says "<key>) Detailed stats", and the key opens a panel (the
//! game's message box) with the effective values ([`weapon_text`], [`ammo_text`]). The key is
//! `[nv-rs Gunplay] iDetailsKey` (a DirectInput key code), rebound on the Settings menu's
//! Controls page ("Detailed Stats Key", [`KEYS`]). The card's DAM and DPS both stay visible (the
//! equipment stats row never cycles them).

use crate::ballistics::{Breakdown, Caliber, Load};
use engine::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

/// The keys the Controls row offers: DirectInput code and name.
pub const KEYS: [(u8, &str); 16] = [
    (0x25, "K"),
    (0x24, "J"),
    (0x26, "L"),
    (0x23, "H"),
    (0x22, "G"),
    (0x16, "U"),
    (0x17, "I"),
    (0x18, "O"),
    (0x19, "P"),
    (0x31, "N"),
    (0x30, "B"),
    (0x2c, "Z"),
    (0x2e, "C"),
    (0x2f, "V"),
    (0xd2, "Insert"),
    (0xcf, "End"),
];

/// The key's name ("K"), or its code.
pub fn key_name(code: i32) -> String {
    KEYS.iter()
        .find(|k| i32::from(k.0) == code)
        .map_or_else(|| format!("key {code}"), |k| k.1.to_string())
}

/// The card's hint line.
pub fn hint(code: i32) -> Vec<u8> {
    format!("{}) Detailed stats", key_name(code)).into_bytes()
}

/// The highlighted item (its `ItemChange`, its form) and the card's display, from the last
/// filling of the card.
static HOVERED: Mutex<Option<(u32, u32, u32)>> = Mutex::new(None);
static KEY_DOWN: AtomicBool = AtomicBool::new(false);

const WEAP: u8 = 0x28;
const AMMO: u8 = 0x29;
const PLAYER: u32 = 0x011d_ea3c;

/// The card was filled for `item` on `display` (from `crate::equipment_stats`'s populate):
/// remembered; the hint line when the item has detailed stats.
pub fn hovered(e: &mut Engine, item: u32, display: u32) -> Option<Vec<u8>> {
    if !crate::gunplay::is_on() || item == 0 || display == 0 {
        *HOVERED.lock().unwrap_or_else(|p| p.into_inner()) = None;
        return None;
    }
    let form = e.mem.u32(item + 8);
    let has = form != 0
        && match e.mem.u8(form + 4) {
            WEAP => crate::ballistics::gun_caliber(e, form).is_some(),
            AMMO => crate::ballistics::caliber_of(&crate::ballistics::name_of(e, form)).is_some(),
            _ => false,
        };
    *HOVERED.lock().unwrap_or_else(|p| p.into_inner()) = has.then_some((item, form, display));
    has.then(|| hint(crate::gunplay::tuning().details_key))
}

/// Once a frame while a card shows (the menus' idle, `crate::equipment_stats`'s fade): the key's
/// press opens the panel for the highlighted item.
pub fn idle(e: &mut Engine) {
    let code = crate::gunplay::tuning().details_key;
    #[cfg(any(windows, target_os = "android", target_os = "linux"))]
    let down = usize::try_from(code)
        .ok()
        .and_then(|k| engine_platform::d3d9real::held_keys().get(k).copied())
        .unwrap_or(false);
    #[cfg(not(any(windows, target_os = "android", target_os = "linux")))]
    let down = {
        let _ = code;
        false
    };
    let was = KEY_DOWN.swap(down, Ordering::Relaxed);
    if !down || was || !crate::gunplay::is_on() {
        return;
    }
    let Some((item, form, display)) = *HOVERED.lock().unwrap_or_else(|p| p.into_inner()) else {
        return;
    };
    if e.mem.u32(item + 8) != form {
        return; // the list changed under it
    }
    let text = match e.mem.u8(form + 4) {
        WEAP => weapon_text(e, item, form, display),
        AMMO => ammo_text(e, form),
        _ => None,
    };
    if let Some(text) = text {
        show(e, &text);
    }
}

/// The game's message box with `text` and an OK button.
fn show(e: &mut Engine, text: &str) {
    const SHOW_MESSAGE_BOX: u32 = 0x0070_3e80;
    let bytes = &text.as_bytes()[..text.len().min(MAX_TEXT)];
    e.with_stack(0x600, |e, frame| {
        let base = frame.addr();
        e.mem.set_cstr(base, bytes);
        let ok = base + 0x5f0;
        e.mem.set_cstr(ok, b"OK");
        let mut words = vec![base, 0, 0, 0, 0, 0x17, 0, 0, ok];
        words.extend([0u32; 9]);
        words.push(0);
        e.call(SHOW_MESSAGE_BOX, &words);
    });
}

/// The panel's text at most.
const MAX_TEXT: usize = 0x5e0;

/// The round's lines (both panels): calibre, velocity and its factors, mass, momentum,
/// projectiles, drop, penetration, transfer.
pub fn round_lines(b: &Breakdown) -> Vec<String> {
    let mut v = vec![format!(
        "Calibre: {} ({}), {}",
        b.caliber.name,
        load_name(b.load),
        b.ammo_name
    )];
    let mut factors = format!(
        "Muzzle velocity: {:.0} m/s = {:.0} calibre x {:.2} this gun ({})",
        b.velocity,
        b.caliber.velocity,
        b.gun,
        match &b.gun_source {
            crate::gun_velocity::Source::Table(id) => format!("table: {id}"),
            crate::gun_velocity::Source::Damage { damage, average } =>
                format!("damage {damage:.0} vs its calibre's {average:.0}"),
            crate::gun_velocity::Source::Default => "no entry".to_string(),
        }
    );
    factors.push_str(&format!(" x {:.2} load", b.load.mods().velocity));
    for (name, f) in &b.mods {
        factors.push_str(&format!(" x {f:.2} {name}"));
    }
    v.push(factors);
    let p = b.mass * b.velocity;
    let into = p * b.share * crate::knockdown::BULLET_FEEL;
    v.push(format!(
        "Bullet: {:.1} g, momentum {p:.2} N s; into a body {into:.0} N s ({:.0}% left in it, x{} \
         game feel): {:.2} m/s on an 80 kg person",
        b.mass * 1000.0,
        b.share * 100.0,
        crate::knockdown::BULLET_FEEL,
        into / 80.0
    ));
    v.push(format!(
        "Projectiles a shot: {}{}",
        b.projectiles,
        if b.pellet {
            " (pellets: they stack)"
        } else {
            ""
        }
    ));
    v.push(format!(
        "Drop: gravity x{:.1}; range {:.0} m",
        b.caliber.drop, b.caliber.range
    ));
    v.push(format!(
        "Penetration: {:.0} power (a body costs {}); DT counts x{:.1}",
        b.power,
        crate::ballistics::BODY_COST,
        b.threshold
    ));
    v
}

fn load_name(load: Load) -> &'static str {
    match load {
        Load::Standard => "standard",
        Load::HollowPoint => "hollow point",
        Load::ArmorPiercing => "armour piercing",
        Load::Jsp => "JSP",
        Load::Surplus => "surplus",
        Load::HandLoad => "hand load",
        Load::Match => "match",
        Load::Incendiary => "incendiary",
        Load::Explosive => "explosive",
        Load::Slug => "slug",
        Load::Magnum => "magnum shell",
    }
}

/// The weapon item's panel (module documentation).
fn weapon_text(e: &mut Engine, item: u32, weapon: u32, display: u32) -> Option<String> {
    let player = e.global::<u32>(PLAYER);
    let b = crate::ballistics::breakdown(e, weapon, item, player)?;
    let name = String::from_utf8_lossy(&jip::commands::the_name(e, weapon)).into_owned();
    let mods = crate::weapon_mods::attached(e, item);
    let has = |effect| mods.iter().any(|m| m.effect == effect);
    let mut lines = vec![name];
    lines.extend(round_lines(&b));
    let card = crate::equipment_stats::card_damage(e, display);
    if !card.is_empty() {
        lines.push(card);
    }
    let rate = e.call(RATE_OF_FIRE, &args![player, weapon, item]).f64() as f32;
    let shots = rate / b.projectiles.max(1) as f32;
    let spread_mod = u32::from(has(crate::weapon_mods::Effect::Spread));
    let min_spread = e.call(MIN_SPREAD, &args![weapon, spread_mod]).f64() as f32;
    let spread = e.call(SPREAD, &args![weapon, spread_mod]).f64() as f32;
    let ap = e.call(AP_COST, &args![weapon]).f64() as f32;
    lines.push(format!(
        "Fire rate: {shots:.1} shots/s; spread {min_spread:.2} min, {spread:.2}; AP {ap:.0}"
    ));
    lines.push(format!(
        "Critical: damage {}, chance x{:.1}",
        e.mem.u16(weapon + CRIT_DAMAGE),
        e.mem.f32(weapon + CRIT_CHANCE)
    ));
    let percent = e.call(ITEM_HEALTH, &args![item, 1u32]).f64() as f32;
    let health = e.call(ITEM_HEALTH, &args![item, 0u32]).f64() as f32;
    let condition_mod = u32::from(
        mods.iter()
            .any(|m| m.effect == crate::weapon_mods::Effect::Other(10)),
    );
    let max = e.call(FORM_HEALTH, &args![weapon, condition_mod]).i32();
    let clip_mod = u32::from(
        mods.iter()
            .any(|m| m.effect == crate::weapon_mods::Effect::Other(2)),
    );
    let clip = e.call(CLIP_ROUNDS, &args![weapon, clip_mod]).u32() & 0xff;
    lines.push(format!(
        "Condition: {percent:.0}% ({health:.0} / {max}); clip {clip}; reload: by its animation (no \
         time in the weapon's data)"
    ));
    let names: Vec<String> = mods
        .iter()
        .filter(|m| m.object != 0)
        .map(|m| crate::ballistics::name_of(e, m.object))
        .collect();
    lines.push(if names.is_empty() {
        "Mods: none attached".to_string()
    } else {
        format!("Mods: {}", names.join(", "))
    });
    Some(lines.join("\n"))
}

/// The ammo item's panel (module documentation).
fn ammo_text(e: &mut Engine, ammo: u32) -> Option<String> {
    let name = crate::ballistics::name_of(e, ammo);
    let caliber: Caliber = crate::ballistics::caliber_of(&name)?;
    let (mult, minus) = crate::ballistics::threshold_effects(e, ammo);
    let load = crate::ballistics::load_of(&name, mult, minus);
    let mods = load.mods();
    let velocity = caliber.velocity * mods.velocity;
    let mass = caliber.mass * mods.mass / 1000.0;
    let pellet = caliber.name.contains("gauge") && load != Load::Slug;
    let share = crate::ballistics::transfer_share(load, pellet);
    let power = caliber.penetration * mods.penetration * mods.velocity * mods.velocity;
    let p = mass * velocity;
    let into = p * share * crate::knockdown::BULLET_FEEL;
    Some(
        [
            name,
            format!("Calibre: {} ({})", caliber.name, load_name(load)),
            format!(
                "Velocity: {velocity:.0} m/s ({:.0} x {:.2} load) in a gun of x1.00",
                caliber.velocity, mods.velocity
            ),
            format!(
                "Mass: {:.1} g{}",
                mass * 1000.0,
                if pellet { " a pellet" } else { "" }
            ),
            format!(
                "Penetration: {power:.0} power (x{:.2} the calibre's; a body costs {})",
                mods.penetration,
                crate::ballistics::BODY_COST
            ),
            format!(
                "Transfer: {:.0}% of its momentum left in a body; DT counts x{:.1}",
                share * 100.0,
                mods.threshold
            ),
            format!(
                "Momentum: {p:.2} N s; into a body {into:.0} N s: {:.2} m/s on an 80 kg person",
                into / 80.0
            ),
        ]
        .join("\n"),
    )
}

// --- The game's words. --------------------------------------------------------------------------

/// `CombatFormulas::GetWeaponRateOfFire` (`cdecl`: owner, weapon, item), the spread getters
/// (`thiscall`, with the spread mod), `ActionPoints::GetWeaponAttackActionPoints`,
/// `ItemChange::GetItemHealth` (as a percentage or not), `TESObjectWEAP::GetFormHealth` (with
/// the condition mod) and `GetFormClipRounds` (with the clip mod); the critical data (`CRDT`:
/// damage, chance multiplier).
const RATE_OF_FIRE: u32 = 0x0064_5d00;
const MIN_SPREAD: u32 = 0x0052_4b80;
const SPREAD: u32 = 0x0052_4be0;
const AP_COST: u32 = 0x0066_dce0;
const ITEM_HEALTH: u32 = 0x004b_cdb0;
const FORM_HEALTH: u32 = 0x004b_cf00;
const CLIP_ROUNDS: u32 = 0x004f_e160;
const CRIT_DAMAGE: u32 = 0x1c0;
const CRIT_CHANCE: u32 = 0x1c4;

// --- The Controls row. --------------------------------------------------------------------------

/// The row's change function (after weighty combat's), the Controls page's mode bit.
pub const CHANGE_FN: u32 = 0x7e06_01f0;
const CONTROLS: u32 = 64;
static OPTION: AtomicU32 = AtomicU32::new(0);

fn index_of(code: i32) -> u32 {
    KEYS.iter()
        .position(|k| i32::from(k.0) == code)
        .unwrap_or(0) as u32
}

fn label(code: i32) -> String {
    format!("Detailed Stats Key: {}", key_name(code))
}

/// The row on the Settings menu's Controls page (from `crate::gunplay::add_rows`): a list of
/// [`KEYS`].
pub fn add_row(e: &mut Engine) {
    use crate::gunplay::{
        heap_text, ARRAY_ADD, LABEL_SIZE, LIST, OPERATOR_NEW, USER_OPTIONS, USER_OPTION_NEW,
        USER_OPTION_SIZE,
    };
    let code = crate::gunplay::tuning().details_key;
    let text = heap_text(e, &label(code), LABEL_SIZE);
    let block = e.call(OPERATOR_NEW, &args![USER_OPTION_SIZE]).u32();
    if text == 0 || block == 0 {
        return;
    }
    let mut call = args![
        block,
        text,
        CHANGE_FN,
        CONTROLS,
        LIST,
        KEYS.len() as u32,
        0u32
    ];
    // The value strings, as the game's own list rows give them (the constructor reads them in
    // reverse).
    for &(_, name) in KEYS.iter().rev() {
        let s = heap_text(e, name, 12);
        if s == 0 {
            return;
        }
        call.push(s);
    }
    let option = e.call(USER_OPTION_NEW, &call).u32();
    OPTION.store(option, Ordering::Relaxed);
    e.with_stack(4, |e, cell| {
        e.mem.set_u32(cell.addr(), option);
        e.call(ARRAY_ADD, &args![USER_OPTIONS, cell]);
    });
}

/// Registers the row's change function.
pub fn install(e: &mut Engine) {
    e.register(CHANGE_FN, |e, a| {
        change(e, Ptr::new(a[0]));
        Ret::default()
    });
}

fn change(e: &mut Engine, option: Ptr) {
    use crate::gunplay::{write_label, DEFAULT_VAL, VAL};
    if option.is_null() {
        return;
    }
    let code = crate::gunplay::tuning().details_key;
    let Some(menu) = crate::gunplay::menu_saving(e) else {
        e.mem.set_u32(option.addr() + VAL, index_of(code));
        e.mem.set_u32(
            option.addr() + DEFAULT_VAL,
            index_of(crate::gunplay::Tuning::PLAY.details_key),
        );
        write_label(e, 0, option, label(code));
        return;
    };
    let index = (e.mem.u32(option.addr() + VAL) as usize).min(KEYS.len() - 1);
    let chosen = i32::from(KEYS[index].0);
    if chosen == code {
        return;
    }
    let mut t = crate::gunplay::tuning();
    t.set("iDetailsKey", chosen);
    crate::gunplay::set_tuning(t);
    write_label(e, menu, option, label(chosen));
    let saved = crate::gunplay::save_key(crate::gunplay::SECTION, "iDetailsKey", chosen);
    eprintln!(
        "Detailed stats: key {}: applied now{}",
        key_name(chosen),
        if saved { ", saved" } else { ", not saved" }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys: unique, named, the default among them; the hint and the label name the key.
    #[test]
    fn keys_and_hint() {
        let mut codes: Vec<u8> = KEYS.iter().map(|k| k.0).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), KEYS.len());
        let default = crate::gunplay::Tuning::PLAY.details_key;
        assert_eq!(index_of(default), 0);
        assert_eq!(hint(default), b"K) Detailed stats".to_vec());
        assert_eq!(key_name(0xd2), "Insert");
        assert!(label(0xd2).len() < crate::gunplay::LABEL_SIZE as usize);
    }

    /// The round's lines carry every factor of the velocity and the momentum.
    #[test]
    fn the_round_is_told_factor_by_factor() {
        let caliber = crate::ballistics::caliber_of(".308 Round").unwrap();
        let b = Breakdown {
            caliber,
            ammo_name: ".308 Round".into(),
            load: Load::Standard,
            gun: 1.1,
            gun_source: crate::gun_velocity::Source::Table("WeapNVSniperRifle".into()),
            mods: vec![("Suppressor".into(), 0.96)],
            velocity: 850.0 * 1.1 * 0.96,
            mass: 0.0113,
            projectiles: 1,
            pellet: false,
            power: 60.0,
            share: 0.6,
            threshold: 1.0,
        };
        let text = round_lines(&b).join("\n");
        assert!(
            text.contains("x 1.10 this gun (table: WeapNVSniperRifle)"),
            "{text}"
        );
        assert!(text.contains("x 0.96 Suppressor"), "{text}");
        assert!(text.contains("Muzzle velocity: 898 m/s"), "{text}");
        assert!(text.contains("11.3 g"), "{text}");
        assert!(text.len() < MAX_TEXT);
    }
}
