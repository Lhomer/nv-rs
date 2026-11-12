//! A perk every level (`NV_PERK_EVERY_LEVEL`), a row of the `fixes` preset ([`crate::fixes`],
//! docs/FIXES.md): **not 1:1 with FalloutNV.exe**, and not a fix of the original at all but a
//! gameplay change the player asked for (Patrick, 2026-11-12); in the `vanilla` preset, or with
//! `bPerkEveryLevel=0`, nothing here runs.
//!
//! **What the original does.** A level-up opens the level-up menu (`LevelUpMenu::Create`,
//! `00784c80`, engine map), which offers the skill points and, on a level that is a multiple of
//! the game setting `iLevelsPerPerk` (the int setting at `011cd074`, its value at `+4` as
//! `0043d4d0` gives it; 2 unless a plugin sets it), a perk. That test is the setting's only reader
//! in the game's code, so the original gives a perk on every second level.
//!
//! **What this does.** `Create` runs as the translation has it, with the setting at 1 while it
//! runs, so every level offers a perk; afterwards it is put back, so whatever else asks for it (a
//! script's `GetGameSetting`) gets the game's value. It is set only when it holds the original's
//! 2: a plugin that sets another value keeps it. Nothing is written to a save, so it applies to
//! every save from its next level-up on; levels gained before are not made up for.

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::fallout_interface::levelupmenu::level_up_menu_create;

/// `LevelUpMenu::Create` (engine map; no words, the tile in `EAX`).
pub const CREATE: u32 = 0x0078_4c80;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "perk every level",
    env: "NV_PERK_EVERY_LEVEL",
    addresses: &[CREATE],
    summary: "a perk is offered at every level-up, not every second one (a gameplay change, not the original's)",
    install,
};

/// `iLevelsPerPerk`: the game setting, and where its value is (`0043d4d0`: `+4`).
const LEVELS_PER_PERK: u32 = 0x011c_d074;
const VALUE: u32 = 4;
/// The original's value, and the one used here.
const ORIGINAL: i32 = 2;
const EVERY_LEVEL: i32 = 1;

/// Registers `Create` at `00784c80`.
pub fn install(e: &mut Engine) {
    e.register(CREATE, |e, _| Ret {
        eax: with_every_level(e, level_up_menu_create),
        ..Ret::default()
    });
}

/// `create` with `iLevelsPerPerk` at 1 while it runs, when it holds the original's 2, and put
/// back afterwards.
fn with_every_level(e: &mut Engine, create: impl FnOnce(&mut Engine) -> u32) -> u32 {
    let value = LEVELS_PER_PERK + VALUE;
    let was = e.mem.i32(value);
    if was != ORIGINAL {
        return create(e);
    }
    e.mem.set_i32(value, EVERY_LEVEL);
    let tile = create(e);
    e.mem.set_i32(value, was);
    tile
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_sees_one_level_per_perk_and_the_setting_is_put_back() {
        let mut e = Engine::new();
        e.mem.map(LEVELS_PER_PERK & !0xfff, 0x1000);
        let value = LEVELS_PER_PERK + VALUE;
        e.mem.set_i32(value, ORIGINAL);
        let tile = with_every_level(&mut e, |e| {
            assert_eq!(e.mem.i32(value), EVERY_LEVEL);
            0x1234
        });
        assert_eq!(tile, 0x1234);
        assert_eq!(e.mem.i32(value), ORIGINAL);
    }

    #[test]
    fn a_plugins_own_value_is_kept() {
        let mut e = Engine::new();
        e.mem.map(LEVELS_PER_PERK & !0xfff, 0x1000);
        let value = LEVELS_PER_PERK + VALUE;
        e.mem.set_i32(value, 3);
        with_every_level(&mut e, |e| {
            assert_eq!(e.mem.i32(value), 3);
            0
        });
        assert_eq!(e.mem.i32(value), 3);
    }
}
