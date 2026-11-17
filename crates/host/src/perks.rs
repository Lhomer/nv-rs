//! nv-rs's own perks (`NV_NEW_PERKS`, a row of the `fixes` preset, [`crate::fixes`], docs/FIXES.md):
//! **not 1:1 with FalloutNV.exe**, five new perks the player asked for (Patrick, 2026-11-17), the
//! numbers in `ragdoll_core::perks`.
//!
//! **Real perk records.** nv-rs writes its own plugin, `Data\nv-rs Perks.esp` ([`PLUGIN`]), made
//! here in code ([`plugin_bytes`]: no file of it is kept anywhere), and adds it to the player's
//! `plugins.txt` ([`with_plugin`]) when the row is on, at start-up before the game reads its plugin
//! list ([`ensure_plugin`]). The game loads it as any plugin: five `PERK` records ([`PERKS`]) with
//! their names, descriptions and requirements (conditions `GetPermanentActorValue`, as the game's
//! own perks have them, and the level in `DATA`), playable, one rank each, so the level-up menu
//! offers them to a player who meets them and the Pip-Boy lists the ones taken. A save made with
//! one of them lists the plugin as it lists any; nv-rs never writes a save. With the row off the
//! plugin stays in the list (a save that has one of these perks keeps it) but nothing of it acts.
//!
//! **Every effect needs the perk** ([`has`]: the player's rank in it, `PlayerCharacter` slot
//! `+0x4a0`, as the level-up menu reads it), resolved from the plugin's place in the load order:
//! - Stopping Power: `crate::knockdown::after_hit` (the player's hits);
//! - Hot Loads: `crate::ballistics` (the player's shots, the round's breakdown);
//! - Bowling Ball: `crate::knockdown::frame` (a body the player knocked down);
//! - Light Touch+: the pickpocket chance (`00643400`, [`pickpocket_chance`]);
//! - Scavenger's Instinct: quick loot (`crate::quick_loot`) and the container menu's items
//!   (`ContainerMenu::InitFunc` `0075d160` and the lists' compare `0075d130`, [`better`]).

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::fallout_interface::containermenu::{container_menu_init_func, fn_0075d130};
use engine::units::unplaced::run_00642860::fn_00643400;
use ragdoll_core::perks as numbers;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The plugin's file name.
pub const PLUGIN: &str = "nv-rs Perks.esp";

/// The pickpocket chance (`00643400`), `ContainerMenu::InitFunc` and the lists' compare.
pub const PICKPOCKET_CHANCE: u32 = 0x0064_3400;
pub const CONTAINER_INIT: u32 = 0x0075_d160;
pub const CONTAINER_COMPARE: u32 = 0x0075_d130;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "nv-rs perks",
    env: "NV_NEW_PERKS",
    addresses: &[PICKPOCKET_CHANCE, CONTAINER_INIT, CONTAINER_COMPARE],
    summary: "five new perks (Stopping Power, Hot Loads, Bowling Ball, Light Touch+, Scavenger's Instinct) in nv-rs's own plugin, nv-rs Perks.esp, written into Data and added to plugins.txt",
    install,
};

/// One of nv-rs's perks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Perk {
    StoppingPower = 0,
    HotLoads = 1,
    BowlingBall = 2,
    LightTouchPlus = 3,
    ScavengersInstinct = 4,
}

/// A perk's record: its form id in the plugin, editor id, name, description, level, and the
/// actor values it needs at least (index, value).
pub struct Def {
    pub perk: Perk,
    pub local: u32,
    pub editor_id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub level: u8,
    pub needs: &'static [(u32, f32)],
}

/// The actor values the requirements read: Strength, Perception, Agility, Guns, Sneak.
const AV_STRENGTH: u32 = 5;
const AV_PERCEPTION: u32 = 6;
const AV_AGILITY: u32 = 10;
const AV_GUNS: u32 = 41;
const AV_SNEAK: u32 = 42;
/// The condition function the game's own perks require skills and SPECIAL with.
const GET_PERMANENT_ACTOR_VALUE: u16 = 495;

/// The five perks.
pub const PERKS: [Def; 5] = [
    Def {
        perk: Perk::StoppingPower,
        local: 0x800,
        editor_id: "NVRSStoppingPower",
        name: "Stopping Power",
        description: "Your rounds hit like a freight train. Every hit you land pushes people 35% harder, so they stagger, stumble and brace more often, and a hit 25% lighter interrupts their attack. Damage is unchanged.",
        level: 12,
        needs: &[(AV_GUNS, 45.0)],
    },
    Def {
        perk: Perk::HotLoads,
        local: 0x801,
        editor_id: "NVRSHotLoads",
        name: "Hot Loads",
        description: "You load every round a little hotter. Every gun you fire has 10% more muzzle velocity: a flatter flight and more push behind each hit. Damage is unchanged.",
        level: 14,
        needs: &[(AV_GUNS, 50.0)],
    },
    Def {
        perk: Perk::BowlingBall,
        local: 0x802,
        editor_id: "NVRSBowlingBall",
        name: "Bowling Ball",
        description: "When you knock someone off their feet, they take others with them. A body you sent flying shoves anyone it crashes into, by how hard it was going: enough to stagger them, and a fast one knocks them down too.",
        level: 10,
        needs: &[(AV_STRENGTH, 6.0)],
    },
    Def {
        perk: Perk::LightTouchPlus,
        local: 0x803,
        editor_id: "NVRSLightTouchPlus",
        name: "Light Touch+",
        description: "Your fingers barely brush a pocket. When you pickpocket, an item's value counts half as much against you, and every attempt is 10% more likely to succeed. The pickpocket menu shows the chance with it.",
        level: 8,
        needs: &[(AV_SNEAK, 50.0), (AV_AGILITY, 6.0)],
    },
    Def {
        perk: Perk::ScavengersInstinct,
        local: 0x804,
        editor_id: "NVRSScavengersInstinct",
        name: "Scavenger's Instinct",
        description: "You can tell a better find at a glance. When you loot, anything that beats what you have equipped in its slot is marked with ^ and listed first.",
        level: 6,
        needs: &[(AV_PERCEPTION, 6.0)],
    },
];

/// The plugin's bytes: the header and the five perk records.
pub fn plugin_bytes() -> Vec<u8> {
    use esm::write::{condition, group, header, record, subrecord, text, Compare};
    let records: Vec<Vec<u8>> = PERKS
        .iter()
        .map(|d| {
            let mut fields = vec![
                text(b"EDID", d.editor_id),
                text(b"FULL", d.name),
                text(b"DESC", d.description),
            ];
            for &(av, value) in d.needs {
                fields.push(condition(
                    GET_PERMANENT_ACTOR_VALUE,
                    av,
                    0,
                    Compare::GreaterOrEqual,
                    value,
                ));
            }
            // Not a trait, the level, one rank, playable, not hidden.
            fields.push(subrecord(b"DATA", &[0, d.level, 1, 1, 0]));
            record(b"PERK", d.local, &fields)
        })
        .collect();
    let next = PERKS.iter().map(|d| d.local).max().unwrap_or(0x7ff) + 1;
    let mut bytes = header(
        records.len() as u32 + 1,
        next,
        "nv-rs",
        "nv-rs's own perks, made by nv-rs (crates/host/src/perks.rs): Stopping Power, Hot Loads, Bowling Ball, Light Touch+, Scavenger's Instinct.",
    );
    bytes.extend_from_slice(&group(b"PERK", &records));
    bytes
}

/// `plugins.txt`'s text with the plugin listed (added at the end, in the file's own line ends),
/// `None` when it is listed already (any case).
pub fn with_plugin(text: &str) -> Option<String> {
    if text.lines().any(|l| {
        l.trim()
            .trim_start_matches('*')
            .eq_ignore_ascii_case(PLUGIN)
    }) {
        return None;
    }
    let end = if text.contains("\r\n") || text.is_empty() {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(end);
    }
    out.push_str(PLUGIN);
    out.push_str(end);
    Some(out)
}

/// Whether the row is on in this run ([`install`]).
static ON: AtomicBool = AtomicBool::new(false);

fn install(e: &mut Engine) {
    ON.store(true, Ordering::Relaxed);
    e.register(PICKPOCKET_CHANCE, |e, a| {
        pickpocket_chance(e, a[0] as i32, a[1] as i32, f32::from_bits(a[2])).into_ret()
    });
    e.register(CONTAINER_INIT, |e, a| {
        container_init(e, a[0], a[1]);
        Ret::default()
    });
    e.register(CONTAINER_COMPARE, |e, a| {
        container_compare(e, a[0], a[1]).into_ret()
    });
}

/// The player's `plugins.txt`: under the game's local application data folder (`CSIDL_LOCAL_APPDATA`
/// as the machine table has it, else `%LOCALAPPDATA%`), `FalloutNV\plugins.txt`.
fn plugins_txt(e: &mut Engine) -> Option<PathBuf> {
    if let Some((_, mut folder)) = engine_platform::d3d9::folder_path_if_loaded(e, 0x1c) {
        folder.extend_from_slice(b"\\FalloutNV\\plugins.txt");
        return Some(engine_platform::files::host_path(
            &engine_platform::files::full_path(e, &folder),
        ));
    }
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(Path::new(&local).join("FalloutNV").join("plugins.txt"))
}

/// The plugin written into `data` (when missing or not the same bytes) and listed in the player's
/// `plugins.txt` (when not listed), with the row on, before the game reads its plugin list.
/// Answers what it did, for the log.
pub fn ensure_plugin(e: &mut Engine, data: &Path) -> Vec<String> {
    let mut said = Vec::new();
    if !ON.load(Ordering::Relaxed) {
        return said;
    }
    let path = data.join(PLUGIN);
    let bytes = plugin_bytes();
    if std::fs::read(&path).ok().as_deref() != Some(&bytes[..]) {
        match std::fs::write(&path, &bytes) {
            Ok(()) => said.push(format!("perks: wrote {}", path.display())),
            Err(why) => said.push(format!("perks: could not write {}: {why}", path.display())),
        }
    }
    match plugins_txt(e) {
        Some(list) => {
            let text = std::fs::read_to_string(&list).unwrap_or_default();
            if let Some(new) = with_plugin(&text) {
                if let Some(parent) = list.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::write(&list, new) {
                    Ok(()) => said.push(format!("perks: {PLUGIN} added to {}", list.display())),
                    Err(why) => {
                        said.push(format!("perks: could not add to {}: {why}", list.display()))
                    }
                }
            }
        }
        None => said.push(format!(
            "perks: no plugins.txt found (no local application data folder): {PLUGIN} not listed"
        )),
    }
    said
}

/// The player (`PlayerCharacter`), `LookupFormByID`, the data handler and its loaded files (their
/// count and array, a file's name), the player's rank in a perk (slot `+0x4a0`, `(perk, 0)`, `AL`).
const PLAYER: u32 = 0x011d_ea3c;
const LOOKUP_FORM: u32 = 0x0048_39c0;
const DATA_HANDLER: u32 = 0x011c_3f2c;
const LOADED_COUNT: u32 = 0x218;
const LOADED_MODS: u32 = 0x21c;
const MOD_NAME: u32 = 0x20;
const PERK_RANK_SLOT: u32 = 0x4a0;

/// The five perks' forms once found (the plugin loaded), in [`Perk`] order.
static FORMS: Mutex<Option<[u32; 5]>> = Mutex::new(None);

/// The plugin's place among the loaded files (its forms' top byte).
fn plugin_index(e: &Engine) -> Option<u32> {
    let handler = e.mem.u32(DATA_HANDLER);
    if handler == 0 {
        return None;
    }
    let count = u32::from(e.mem.u8(handler + LOADED_COUNT));
    (0..count).find(|&i| {
        let info = e.mem.u32(handler + LOADED_MODS + 4 * i);
        info != 0
            && e.mem
                .cstr(info + MOD_NAME)
                .eq_ignore_ascii_case(PLUGIN.as_bytes())
    })
}

/// The perk's form, once the plugin is loaded.
fn form(e: &mut Engine, perk: Perk) -> Option<u32> {
    if let Some(forms) = *FORMS.lock().unwrap_or_else(|p| p.into_inner()) {
        return Some(forms[perk as usize]);
    }
    let index = plugin_index(e)?;
    let mut forms = [0u32; 5];
    for d in &PERKS {
        let id = (index << 24) | d.local;
        forms[d.perk as usize] = e.call(LOOKUP_FORM, &args![id]).u32();
    }
    if forms.contains(&0) {
        return None;
    }
    *FORMS.lock().unwrap_or_else(|p| p.into_inner()) = Some(forms);
    Some(forms[perk as usize])
}

/// Whether the player has `perk` (the row on, the plugin loaded, a rank in it).
pub fn has(e: &mut Engine, perk: Perk) -> bool {
    if !ON.load(Ordering::Relaxed) {
        return false;
    }
    let player = e.global::<u32>(PLAYER);
    if player == 0 {
        return false;
    }
    let Some(form) = form(e, perk) else {
        return false;
    };
    e.vcall(player, PERK_RANK_SLOT, &args![form, 0u32]).u32() & 0xff != 0
}

/// Whether `actor` is the player and has `perk`.
pub fn player_has(e: &mut Engine, actor: u32, perk: Perk) -> bool {
    actor != 0 && actor == e.global::<u32>(PLAYER) && has(e, perk)
}

// --- Light Touch+ -----------------------------------------------------------------------------

/// The settings `00643400` weighs its terms with and its limits (`Setting` objects; their float
/// at the address `00403e20` gives).
const SETTING_VALUE: u32 = 0x0040_3e20;
const PICKPOCKET_FIRST: (u32, u32) = (0x011c_def8, 0x011c_d8e4);
const PICKPOCKET_SECOND: (u32, u32) = (0x011c_d48c, 0x011c_dec8);
const PICKPOCKET_THIRD: (u32, u32) = (0x011c_da88, 0x011c_d8a8);
const PICKPOCKET_MINIMUM: u32 = 0x011c_dffc;
const PICKPOCKET_MAXIMUM: u32 = 0x011c_da40;

fn setting(e: &mut Engine, setting: u32) -> f64 {
    let at = e.call(SETTING_VALUE, &args![setting]).u32();
    if at == 0 {
        0.0
    } else {
        f64::from(e.mem.f32(at))
    }
}

/// The pickpocket chance (`00643400`: the player's and the target's Sneak and the item's value,
/// `PickpocketCheck`'s roll and the pickpocket menu's line both read it): the game's own, or with
/// Light Touch+ the value's penalty halved and 10 added (`ragdoll_core::perks::light_touch_chance`).
/// The game's chance has no weight term: the value is all the item counts for.
fn pickpocket_chance(e: &mut Engine, player: i32, target: i32, value: f32) -> i32 {
    if !has(e, Perk::LightTouchPlus) {
        return fn_00643400(e, player, target, value);
    }
    let player_term =
        f64::from(player) * setting(e, PICKPOCKET_FIRST.1) + setting(e, PICKPOCKET_FIRST.0);
    let target_term =
        f64::from(target) * setting(e, PICKPOCKET_SECOND.1) + setting(e, PICKPOCKET_SECOND.0);
    let (scale, add) = (
        setting(e, PICKPOCKET_THIRD.1),
        setting(e, PICKPOCKET_THIRD.0),
    );
    let (low, high) = (
        setting(e, PICKPOCKET_MINIMUM).trunc(),
        setting(e, PICKPOCKET_MAXIMUM).trunc(),
    );
    numbers::light_touch_chance(player_term, target_term, value, scale, add, low, high)
}

// --- Scavenger's Instinct ---------------------------------------------------------------------

/// An item's worth against the equipped (`ragdoll_core::perks::Score`): a weapon's force (the
/// form's own stats for loot, the player's held one with its mods for the equipped), apparel's
/// damage threshold and resistance (`DNAM`: DR a word at `+0x178`, DT a float at `+0x17c`).
fn score(e: &mut Engine, item: u32, equipped: bool) -> Option<numbers::Score> {
    use crate::equipment_stats::Kind;
    let form = e.mem.u32(item + 8);
    if form == 0 {
        return None;
    }
    match crate::equipment_stats::kind_of(e, item) {
        Kind::Weapon => {
            let stats = if equipped {
                let player = e.global::<u32>(PLAYER);
                crate::gunplay::effective_stats(e, player, form)
            } else {
                crate::gunplay::weapon_stats(e, form)
            }?;
            Some(numbers::Score::Weapon(crate::gunplay::force(&stats)))
        }
        Kind::Apparel(_) => {
            let dr = f32::from(e.mem.u16(form + 0x178) as i16);
            let dt = e.mem.f32(form + 0x17c);
            Some(numbers::Score::Apparel {
                dt: if dt.is_finite() { dt } else { 0.0 },
                dr,
            })
        }
        Kind::Other => None,
    }
}

/// Whether the inventory item `item` (an entry: its form at `+8`) beats every item the player has
/// equipped in its slot, with Scavenger's Instinct (`ragdoll_core::perks::beats`).
pub fn better(e: &mut Engine, item: u32) -> bool {
    if item == 0 || !has(e, Perk::ScavengersInstinct) {
        return false;
    }
    let Some(own) = score(e, item, false) else {
        return false;
    };
    let kind = crate::equipment_stats::kind_of(e, item);
    let worn = crate::equipment_stats::equipped_for(e, kind);
    let mut scores = Vec::new();
    for &w in &worn {
        if let Some(s) = score(e, w, true) {
            scores.push(s);
        }
        e.call(crate::equipment_stats::ITEM_DELETE, &args![w, 1u32]);
    }
    numbers::beats(own, &scores)
}

/// The marker in front of a better item's name.
pub const MARKER: &[u8] = b"^ ";

/// `name` with the marker in front.
pub fn marked(name: &[u8]) -> Vec<u8> {
    let mut out = MARKER.to_vec();
    out.extend_from_slice(name);
    out
}

/// The container menu, its mode (`+0x80`: 1 a container or a corpse), a tile's string trait and
/// the list item's second tile (`007b5370` on the item + 4).
const CONTAINER_MENU: u32 = 0x011d_93f8;
const CONTAINER_MODE: u32 = 0x80;
const TILE_GET_STRING: u32 = 0x00a0_11f0;
const LIST_ITEM_TILE: u32 = 0x007b_5370;

fn looting(e: &Engine) -> bool {
    let menu = e.global::<u32>(CONTAINER_MENU);
    menu != 0 && e.mem.i32(menu + CONTAINER_MODE) == 1
}

fn tile_text(e: &mut Engine, tile: u32) -> Vec<u8> {
    if tile == 0 {
        return Vec::new();
    }
    let text = e
        .call(
            TILE_GET_STRING,
            &args![tile, crate::menu_tiles::TRAIT_STRING as u32],
        )
        .u32();
    if text == 0 {
        Vec::new()
    } else {
        e.mem.cstr(text)
    }
}

/// `ContainerMenu::InitFunc` as the game's, then, looting with Scavenger's Instinct, a better
/// item's name marked (both of its tiles).
fn container_init(e: &mut Engine, tile: u32, item: u32) {
    container_menu_init_func(e, tile, item);
    if !looting(e) || !better(e, item) {
        return;
    }
    let text = tile_text(e, tile);
    if text.starts_with(MARKER) {
        return;
    }
    let text = marked(&text);
    crate::menu_tiles::set_text(e, tile, crate::menu_tiles::TRAIT_STRING, &text);
    let slot = e.call(LIST_ITEM_TILE, &args![tile.wrapping_add(4)]).u32();
    let second = if slot == 0 { 0 } else { e.mem.u32(slot) };
    crate::menu_tiles::set_text(e, second, crate::menu_tiles::TRAIT_STRING, &text);
}

/// The lists' compare as the game's, except that, looting with Scavenger's Instinct, a marked item
/// comes before an unmarked one.
fn container_compare(e: &mut Engine, first: u32, second: u32) -> i32 {
    if looting(e) && has(e, Perk::ScavengersInstinct) {
        let a = tile_text(e, e.mem.u32(first)).starts_with(MARKER);
        let b = tile_text(e, e.mem.u32(second)).starts_with(MARKER);
        if a != b {
            return if a { -1 } else { 1 };
        }
    }
    fn_0075d130(e, first, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plugin reads back as five playable perks of one rank: their names, descriptions,
    /// levels and requirements.
    #[test]
    fn the_plugin_holds_the_five_perks() {
        let plugin = esm::Plugin::from_bytes(plugin_bytes()).unwrap();
        let perks: Vec<_> = plugin.records_of_type(esm::FourCC(*b"PERK")).collect();
        assert_eq!(perks.len(), 5);
        for (entry, def) in perks.iter().zip(&PERKS) {
            assert_eq!(entry.header.form_id.0, def.local);
            let field = |k: &[u8; 4]| {
                plugin
                    .subrecord_of(entry, esm::FourCC(*k))
                    .unwrap()
                    .unwrap()
            };
            assert_eq!(&field(b"FULL")[..], [def.name.as_bytes(), b"\0"].concat());
            assert!(field(b"DESC").len() > 40);
            assert_eq!(&field(b"DATA")[..], &[0, def.level, 1, 1, 0]);
            let conditions: Vec<Vec<u8>> = plugin
                .subrecords(entry)
                .unwrap()
                .into_iter()
                .filter(|s| s.kind == esm::FourCC(*b"CTDA"))
                .map(|s| s.data.to_vec())
                .collect();
            assert_eq!(conditions.len(), def.needs.len());
            for (c, &(av, value)) in conditions.iter().zip(def.needs) {
                assert_eq!(u16::from_le_bytes([c[8], c[9]]), 495);
                assert_eq!(u32::from_le_bytes([c[12], c[13], c[14], c[15]]), av);
                assert_eq!(f32::from_le_bytes([c[4], c[5], c[6], c[7]]), value);
                assert_eq!(c[0] >> 5, 3, ">=");
            }
        }
        let levels: Vec<u8> = PERKS.iter().map(|d| d.level).collect();
        assert_eq!(levels, vec![12, 14, 10, 8, 6]);
    }

    /// The plugin is added to `plugins.txt` once, at the end, in the file's line ends.
    #[test]
    fn the_plugin_is_listed_once() {
        let text = "FalloutNV.esm\r\nDeadMoney.esm\r\n";
        let new = with_plugin(text).unwrap();
        assert_eq!(new, "FalloutNV.esm\r\nDeadMoney.esm\r\nnv-rs Perks.esp\r\n");
        assert_eq!(with_plugin(&new), None);
        assert_eq!(with_plugin("FalloutNV.esm\nNV-RS PERKS.ESP\n"), None);
        assert_eq!(
            with_plugin("FalloutNV.esm").unwrap(),
            "FalloutNV.esm\nnv-rs Perks.esp\n"
        );
        assert_eq!(with_plugin("").unwrap(), "nv-rs Perks.esp\r\n");
    }

    /// Without the row (or the plugin, or the player) no perk is had, and nothing acts.
    #[test]
    fn no_perk_without_the_plugin() {
        let mut e = Engine::new();
        e.mem.map(0x011c_3000, 0x1000);
        e.mem.map(0x011d_e000, 0x1000);
        for d in &PERKS {
            assert!(!has(&mut e, d.perk));
        }
    }

    /// Light Touch+ (the pickpocket chance `00643400`, which the roll and the menu's line read): the
    /// game's own without the perk; with it, the value's penalty halved and 10 added. A game in
    /// memory: the plugin loaded fourth, the player's rank in the perk from its slot `+0x4a0`.
    #[test]
    fn light_touch_changes_the_chance_only_with_the_perk() {
        let mut e = Engine::new();
        e.mem.map(0x011c_0000, 0x40000);
        e.mem.map(0x0020_0000, 0x10000);
        // The data handler: four loaded files, this plugin the fourth (index 3).
        let handler = 0x0020_0000u32;
        e.mem.set_u32(DATA_HANDLER, handler);
        e.mem.set_u8(handler + LOADED_COUNT, 4);
        for i in 0..4u32 {
            let info = 0x0020_1000 + i * 0x100;
            e.mem.set_u32(handler + LOADED_MODS + 4 * i, info);
            let name: &[u8] = if i == 3 {
                PLUGIN.as_bytes()
            } else {
                b"Other.esm"
            };
            e.mem.set_cstr(info + MOD_NAME, name);
        }
        e.register_double(LOOKUP_FORM, |_, a| {
            (0x0030_0000 | (a[0] & 0xfff)).into_ret()
        });
        // The player: its rank in a perk from slot +0x4a0, 1 for Light Touch+'s form.
        let (player, vtable, rank_fn) = (0x0020_4000u32, 0x0020_5000u32, 0x0020_6000u32);
        e.mem.set_u32(PLAYER, player);
        e.mem.set_u32(player, vtable);
        e.mem.set_u32(vtable + PERK_RANK_SLOT, rank_fn);
        let light_touch = 0x0030_0000 | PERKS[Perk::LightTouchPlus as usize].local;
        let held = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let has_it = held.clone();
        e.register_double(rank_fn, move |_, a| {
            u32::from(a[1] == light_touch && has_it.load(Ordering::Relaxed)).into_ret()
        });
        // The settings: Sneak x1 + 0 for the player, x-1 + 0 for the target, the value x-0.1 + 0,
        // the chance between 5 and 95; each setting's float at the address 00403e20 gives.
        let values: [(u32, f32); 8] = [
            (PICKPOCKET_FIRST.0, 0.0),
            (PICKPOCKET_FIRST.1, 1.0),
            (PICKPOCKET_SECOND.0, 0.0),
            (PICKPOCKET_SECOND.1, -1.0),
            (PICKPOCKET_THIRD.0, 0.0),
            (PICKPOCKET_THIRD.1, -0.1),
            (PICKPOCKET_MINIMUM, 5.0),
            (PICKPOCKET_MAXIMUM, 95.0),
        ];
        for (setting, value) in values {
            e.mem.set_f32(setting + 4, value);
        }
        e.register_double(SETTING_VALUE, |_, a| (a[0] + 4).into_ret());
        install(&mut e);
        *FORMS.lock().unwrap() = None;
        // Sneak 60 against 20, a 200-cap item: 60 - 20 - 20 = 20 without; 60 - 20 - 10 + 10 = 40
        // with (each term stored as a float, as the game stores it).
        let chance = |e: &mut Engine| {
            e.call(PICKPOCKET_CHANCE, &args![60u32, 20u32, 200.0f32])
                .i32()
        };
        let plain = chance(&mut e);
        held.store(true, Ordering::Relaxed);
        let with = chance(&mut e);
        assert_eq!(plain, 20, "without");
        assert_eq!(with, 40, "with Light Touch+");
        assert!(has(&mut e, Perk::LightTouchPlus));
        assert!(!has(&mut e, Perk::StoppingPower));
        *FORMS.lock().unwrap() = None;
    }

    /// The marker goes in front of a better item's name.
    #[test]
    fn a_better_item_is_marked() {
        assert_eq!(marked(b"Hunting Rifle"), b"^ Hunting Rifle".to_vec());
        assert!(marked(b"x").starts_with(MARKER));
    }
}
