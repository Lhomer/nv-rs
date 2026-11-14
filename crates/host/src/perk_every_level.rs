//! Perks per level (`NV_PERK_EVERY_LEVEL`), a row of the `fixes` preset ([`crate::fixes`],
//! docs/FIXES.md): **not 1:1 with FalloutNV.exe**, and not a fix of the original at all but a
//! gameplay change the player asked for (Patrick, 2026-11-12: a perk every level; 2026-11-14: two
//! perks every level, as a setting); in the `vanilla` preset, or with `bPerkEveryLevel=0`, nothing
//! here runs.
//!
//! **What the original does.** A level-up opens the level-up menu (`LevelUpMenu::Create`,
//! `00784c80`, engine map), which offers the skill points and, on a level that is a multiple of
//! the game setting `iLevelsPerPerk` (the int setting at `011cd074`, its value at `+4` as
//! `0043d4d0` gives it; 2 unless a plugin sets it), the perks the player qualifies for, of which
//! one may be chosen (`iMaxPoints[1]` is 1; a click on a perk replaces the one chosen,
//! `LevelUpMenu::DoClick`, `00785aa0`). Closing the menu gives every chosen perk.
//!
//! **What this does.** `[nv-rs Gameplay] sPerksPerLevel` (`NV_PERKS_PER_LEVEL`; the Settings
//! menu's Gameplay page, "Perks per Level", which saves it), from 0.5 (the original's one every
//! second level) to 3 in halves, 2 by default. A level gets `floor(level x rate) - floor((level - 1)
//! x rate)` perks ([`perks_at`]): 1.5 gives 2 and 1 in turn, 0.5 one on each even level. `Create`
//! runs as the translation has it with `iLevelsPerPerk` at 1 on a level with perks (so the perks
//! are offered, whatever a plugin set it to) and out of reach on one without, put back afterwards;
//! then the perk page's `iMaxPoints[1]` is the level's count (never more than the perks offered,
//! so the menu can always be finished). On that page a click on a perk adds it to the chosen ones
//! while fewer than the count are chosen (the original's replace once the count is reached, and a
//! click on a chosen one unchooses it), so the perks are picked one after another on the same page
//! and all given on closing. Perks of plugins and DLC are offered as the game offers them; traits
//! are chosen at character creation and are not touched. Nothing is written to a save: it applies
//! from a save's next level-up on, and levels gained before are not made up for.

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::fallout_interface::levelupmenu::{
    fn_00787760, fn_00787780, fn_007877a0, fn_00787850, fn_007878d0,
    level_up_menu_apply_perk_effects, level_up_menu_create, level_up_menu_do_click,
    level_up_menu_update_point_counter,
};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// `LevelUpMenu::Create` (engine map; no words, the tile in `EAX`) and `LevelUpMenu::DoClick`
/// (`thiscall`: the id, the tile).
pub const CREATE: u32 = 0x0078_4c80;
pub const DO_CLICK: u32 = 0x0078_5aa0;
/// The Settings row's change function (after weighty combat's, `crate::gunplay::CHANGE_FNS`).
pub const CHANGE_FN: u32 = 0x7e06_01d0;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "perk every level",
    env: "NV_PERK_EVERY_LEVEL",
    addresses: &[CREATE, DO_CLICK],
    summary: "perks per level as Settings > Gameplay sets it (2 by default, picked one after another on the level-up menu's perk page; 0.5 is the original's one every second level), not the original's (a gameplay change)",
    install,
};

/// The section and key of the setting, and its rate in halves: the default (2 a level), the
/// lowest (0.5, the original's) and the highest (3).
pub const SECTION: &str = "nv-rs Gameplay";
pub const KEY: &str = "sPerksPerLevel";
pub const DEFAULT_HALVES: u32 = 4;
pub const MIN_HALVES: u32 = 1;
pub const MAX_HALVES: u32 = 6;

/// `iLevelsPerPerk`: the game setting, and where its value is (`0043d4d0`: `+4`).
const LEVELS_PER_PERK: u32 = 0x011c_d074;
const VALUE: u32 = 4;
/// The player, its level (`0087f9f0`, a 16-bit word), `LevelUpMenu::pMe`, the menu's page, points
/// (`iCurrPoints`, `iMaxPoints`, per page), perk list and offered perks, a list's count, the user
/// traits of the points and a line's alpha, full alpha, a tile's int setter and float getter.
const PLAYER: u32 = 0x011d_ea3c;
const PLAYER_LEVEL: u32 = 0x0087_f9f0;
const LEVEL_UP_MENU_ME: u32 = 0x011d_9fdc;
const CURRENT_PAGE: u32 = 0x28;
const CURR_POINTS: u32 = 0x54;
const MAX_POINTS: u32 = 0x5c;
const PERKS_LIST: u32 = 0x94;
const AVAILABLE_PERKS: u32 = 0xc4;
const LIST_COUNT: u32 = 0x005a_e380;
const UT_CURR_POINTS: u32 = 0x011d_9fd8;
const UT_TEXT_ALPHA: u32 = 0x011d_9ff4;
const FULL_ALPHA: u32 = 0x0101_e568;
const TILE_SET_INT: u32 = 0x0070_0320;
const TILE_GET_FLOAT: u32 = 0x00a0_11b0;
/// The perk page, and a perk line's click id.
const PERK_PAGE: u32 = 1;
const PERK_CLICK: u32 = 0xc;

/// The rate in halves in force.
static HALVES: AtomicU32 = AtomicU32::new(DEFAULT_HALVES);
/// The row was installed (its Settings row is made then).
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// The rate the run starts with (`crate::fixes::install`).
pub fn set_rate(halves: u32) {
    HALVES.store(halves.clamp(MIN_HALVES, MAX_HALVES), Ordering::Relaxed);
}

pub fn rate() -> u32 {
    HALVES.load(Ordering::Relaxed)
}

/// `sPerksPerLevel` as the file or `NV_PERKS_PER_LEVEL` holds it, in halves: 0.5 to 3 (a value
/// between halves is taken to the nearest); `None` when it isn't a number in that range.
pub fn parse_rate(raw: &str) -> Option<u32> {
    let v: f32 = raw.trim().parse().ok()?;
    if !v.is_finite() || !(0.5..=3.0).contains(&v) {
        return None;
    }
    Some(((v * 2.0).round() as u32).clamp(MIN_HALVES, MAX_HALVES))
}

/// The rate as the file holds it: `2`, `1.5`, `0.5`.
pub fn format_rate(halves: u32) -> String {
    if halves % 2 == 0 {
        format!("{}", halves / 2)
    } else {
        format!("{}.5", halves / 2)
    }
}

/// The perks level `level` gets at `halves` a level: `floor(level x rate) - floor((level - 1) x
/// rate)`.
pub fn perks_at(level: u32, halves: u32) -> u32 {
    if level == 0 {
        return 0;
    }
    let before = (level - 1) * halves / 2;
    level * halves / 2 - before
}

/// `iLevelsPerPerk` for `Create` on a level with `perks` perks: 1 (offered), or out of reach.
pub fn levels_per_perk(perks: u32) -> i32 {
    if perks > 0 {
        1
    } else {
        i32::MAX
    }
}

/// Registers `Create`, `DoClick` and the Settings row's change function.
pub fn install(e: &mut Engine) {
    INSTALLED.store(true, Ordering::Relaxed);
    e.register(CREATE, |e, _| Ret {
        eax: create(e),
        ..Ret::default()
    });
    e.register(DO_CLICK, |e, a| {
        let (this, id, tile) = (a[0], a[1], a[2]);
        if id == PERK_CLICK
            && this != 0
            && e.mem.u32(this + CURRENT_PAGE) == PERK_PAGE
            && e.mem.u32(this + MAX_POINTS + 4 * PERK_PAGE) > 1
        {
            click_perk(e, this, tile);
        } else {
            level_up_menu_do_click(e, Ptr::new(this), id, tile);
        }
        Ret::default()
    });
    e.register(CHANGE_FN, |e, a| {
        change(e, Ptr::new(a[0]));
        Ret::default()
    });
}

/// `Create` with the level's perks offered, and as many to choose (module documentation).
fn create(e: &mut Engine) -> u32 {
    let player = e.global::<u32>(PLAYER);
    let level = if player == 0 {
        0
    } else {
        e.call(PLAYER_LEVEL, &args![player]).u32() & 0xffff
    };
    let perks = perks_at(level, rate());
    let value = LEVELS_PER_PERK + VALUE;
    let was = e.mem.i32(value);
    e.mem.set_i32(value, levels_per_perk(perks));
    let tile = level_up_menu_create(e);
    e.mem.set_i32(value, was);
    let me = e.mem.u32(LEVEL_UP_MENU_ME);
    let offered = if me == 0 {
        0
    } else {
        e.call(LIST_COUNT, &args![me + AVAILABLE_PERKS]).u32()
    };
    if me != 0 && perks > 1 && offered > 0 {
        e.mem
            .set_u32(me + MAX_POINTS + 4 * PERK_PAGE, perks.min(offered));
    }
    eprintln!(
        "Perks per level: level {level}, {} a level: {perks} perk(s) to choose ({offered} offered; iLevelsPerPerk was {was})",
        format_rate(rate())
    );
    tile
}

/// A click on a perk line of the perk page when more than one may be chosen: a chosen one is
/// unchosen; another (fully shown, so offered) is added while fewer than the count are chosen,
/// else replaces them as the original does; its effects are applied and the counter updated.
fn click_perk(e: &mut Engine, this: u32, tile: u32) {
    let perks = Ptr::new(this + PERKS_LIST);
    if fn_007877a0(e, perks, tile) {
        fn_00787780(e, perks, tile);
        level_up_menu_apply_perk_effects(e, Ptr::new(this), tile);
    } else {
        let alpha_trait = e.mem.u32(UT_TEXT_ALPHA);
        let alpha = e.call(TILE_GET_FLOAT, &args![tile, alpha_trait]).f64();
        let full: f64 = e.global(FULL_ALPHA);
        if alpha == full {
            let max = e.mem.u32(this + MAX_POINTS + 4 * PERK_PAGE);
            if fn_007878d0(e, perks) >= max {
                fn_00787850(e, perks, 1);
            }
            fn_00787760(e, perks, tile);
            level_up_menu_apply_perk_effects(e, Ptr::new(this), tile);
        }
    }
    let chosen = fn_007878d0(e, perks);
    e.mem.set_u32(this + CURR_POINTS + 4 * PERK_PAGE, chosen);
    let points_trait = e.mem.u32(UT_CURR_POINTS);
    let root = e.mem.u32(this + 4);
    if root != 0 {
        e.call(TILE_SET_INT, &args![root, points_trait, chosen]);
    }
    level_up_menu_update_point_counter(e, Ptr::new(this));
}

// --- The Settings row (Gameplay page). --------------------------------------------------------

/// The Gameplay page's mode bit.
const GAMEPLAY: u32 = 8;

fn label(halves: u32) -> String {
    if halves == MIN_HALVES {
        "Perks per Level: 0.5 (original)".to_string()
    } else {
        format!("Perks per Level: {}", format_rate(halves))
    }
}

/// The row, on the Settings menu's Gameplay page (from `crate::gunplay::add_rows`): a meter of the
/// six rates.
pub fn add_row(e: &mut Engine) {
    use crate::gunplay::{
        heap_text, ARRAY_ADD, LABEL_SIZE, METER, OPERATOR_NEW, USER_OPTIONS, USER_OPTION_NEW,
        USER_OPTION_SIZE,
    };
    if !INSTALLED.load(Ordering::Relaxed) {
        return;
    }
    let text = heap_text(e, &label(rate()), LABEL_SIZE);
    let block = e.call(OPERATOR_NEW, &args![USER_OPTION_SIZE]).u32();
    if text == 0 || block == 0 {
        return;
    }
    let count = MAX_HALVES - MIN_HALVES + 1;
    let option = e
        .call(
            USER_OPTION_NEW,
            &args![block, text, CHANGE_FN, GAMEPLAY, METER, count, 0u32],
        )
        .u32();
    e.with_stack(4, |e, cell| {
        e.mem.set_u32(cell.addr(), option);
        e.call(ARRAY_ADD, &args![USER_OPTIONS, cell]);
    });
}

/// The row's change function: the menu reads the value, or the player moved it (applied at once,
/// saved in a play run).
fn change(e: &mut Engine, option: Ptr) {
    use crate::gunplay::{write_label, DEFAULT_VAL, VAL};
    if option.is_null() {
        return;
    }
    let Some(menu) = crate::gunplay::menu_saving(e) else {
        e.mem.set_u32(option.addr() + VAL, rate() - MIN_HALVES);
        e.mem
            .set_u32(option.addr() + DEFAULT_VAL, DEFAULT_HALVES - MIN_HALVES);
        write_label(e, 0, option, label(rate()));
        return;
    };
    let halves = (e.mem.u32(option.addr() + VAL) + MIN_HALVES).clamp(MIN_HALVES, MAX_HALVES);
    if halves == rate() {
        return;
    }
    set_rate(halves);
    write_label(e, menu, option, label(halves));
    let saved = save(halves);
    eprintln!(
        "Perks per level: {KEY} {}: applied from the next level-up{}",
        format_rate(halves),
        if saved { ", saved" } else { ", not saved" }
    );
}

/// The rate written to the run's `Fallout.ini` (a launcher's play run only).
fn save(halves: u32) -> bool {
    if !crate::fixes::is_play_run(|name| std::env::var(name).ok()) {
        return false;
    }
    let text = format_rate(halves);
    crate::game_ini::path().is_some_and(|path| {
        crate::game_ini::write_keys(&path, &[(SECTION, KEY, text.as_str())]).is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two a level gives two at every level; 0.5 one on each even level (the original); 1.5 two
    /// and one in turn; 3 three.
    #[test]
    fn perks_by_level_and_rate() {
        for level in 1..20 {
            assert_eq!(perks_at(level, 4), 2, "{level}");
            assert_eq!(perks_at(level, 2), 1);
            assert_eq!(perks_at(level, 6), 3);
            assert_eq!(perks_at(level, 1), u32::from(level % 2 == 0));
        }
        assert_eq!(perks_at(2, 3), 2);
        assert_eq!(perks_at(3, 3), 1);
        assert_eq!(perks_at(0, 4), 0);
        let total: u32 = (1..=30).map(|l| perks_at(l, 3)).sum();
        assert_eq!(total, 45, "1.5 a level over 30 levels");
    }

    /// The setting reads as halves from 0.5 to 3, and is written back the same.
    #[test]
    fn the_rate_reads_and_writes_in_halves() {
        assert_eq!(parse_rate("2"), Some(4));
        assert_eq!(parse_rate(" 1.5 "), Some(3));
        assert_eq!(parse_rate("0.5"), Some(1));
        assert_eq!(parse_rate("3"), Some(6));
        assert_eq!(parse_rate("1.7"), Some(3));
        assert_eq!(parse_rate("0.4"), None);
        assert_eq!(parse_rate("4"), None);
        assert_eq!(parse_rate("two"), None);
        for h in MIN_HALVES..=MAX_HALVES {
            assert_eq!(parse_rate(&format_rate(h)), Some(h));
        }
        assert_eq!(format_rate(4), "2");
        assert_eq!(format_rate(1), "0.5");
        assert!(label(MAX_HALVES).len() < crate::gunplay::LABEL_SIZE as usize);
        assert!(label(MIN_HALVES).len() < crate::gunplay::LABEL_SIZE as usize);
    }

    /// A level with perks offers them; one without has them out of reach.
    #[test]
    fn the_game_setting_offers_or_not() {
        assert_eq!(levels_per_perk(2), 1);
        assert_eq!(levels_per_perk(1), 1);
        for level in 1..100 {
            assert_ne!(level % levels_per_perk(0), 0);
        }
    }
}
