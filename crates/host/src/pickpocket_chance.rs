//! Pickpocket chance (`NV_PICKPOCKET_CHANCE`), a row of the `fixes` preset ([`crate::fixes`],
//! docs/FIXES.md): **not 1:1 with FalloutNV.exe**, an nv-rs addition the player asked for
//! (Patrick, 2026-11-12: "When I'm in the pickpocket menu, display the percentage chance of
//! successfully pickpocketing"). In the `vanilla` preset, or with `bPickpocketChance=0`, the menu
//! is the original's.
//!
//! **What the player sees.** In the container menu's pickpocket mode (`ContainerMenu`,
//! `eContainerMode` 2, an actor's inventory), "Chance: 75%" on the right of the target's title line:
//! the chance that taking the highlighted item (from their list) or placing it (from the player's)
//! succeeds. It follows the highlight, the list, and the count: one item, the whole stack for what
//! the menu takes whole (gold and weightless items taken from them, `ContainerMenu::DoClick`), and
//! the number chosen while the quantity menu is open. It is written in the title's own font and
//! colors. Ordinary containers, corpses, bartering and companions show nothing.
//!
//! **The number is the game's.** The steps `ContainerMenu::ItemSelectCallback` (`0075dc80`) takes
//! before its `PickpocketCheck` ([`check`]: what counts, and the value checked: the stolen value
//! `ContainerMenu::GetStolenValue` (`0075e240`) of the item and count when taking, 1 when
//! placing), then `PickpocketCheck`'s (`0075e0b0`) own inputs: no check (100%) without an actor,
//! for a value of 0 or less, or when the target's slot `+0x230` says so; else the chance formula
//! `00643400` of the player's clamped actor value `0x2a` (`0066ef20` on the player `+0xa4`), the
//! target's actor value `0x2a` (slot `+8` on the target `+0xa4`) and the value as a float. The take
//! succeeds when `BSRandom::UnsignedInt(100)` is below that chance (unsigned), so the success
//! percentage is the chance, at most 100 ([`success_percent`]). Nothing is rolled here and nothing
//! is changed: not the skill use (`+0x47c`), the tally, the caught state, the message or karma.
//!
//! **Where it runs.** After `ContainerMenu::DoIdle` (`0075eac0`), which the interface calls for
//! every open menu each frame (`InterfaceManager`'s menu loop, slot `+0x2c`), the quantity menu's
//! time included. The text is one tile of nv-rs's own ([`crate::menu_tiles::read_xml`]), made a
//! sibling of the target's title tile (`ContainerMenu +0x40`), so it sits in the title's frame.

use crate::fixes::Fix;
use crate::menu_tiles::{
    child, is_child, read_xml, set_text, set_value, set_visible, trait_value, TRAIT_STRING,
    TRAIT_WIDTH, TRAIT_X,
};
use engine::prelude::*;
use engine::units::bsmenu::tile::{fn_00a011b0, fn_00a013d0};
use engine::units::fallout_interface::containermenu::container_menu_do_idle;
use std::sync::Mutex;

/// `ContainerMenu::DoIdle` (`0075eac0`).
pub const DO_IDLE: u32 = 0x0075_eac0;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "pickpocket chance",
    env: "NV_PICKPOCKET_CHANCE",
    addresses: &[DO_IDLE],
    summary:
        "the pickpocket menu shows the game's chance of taking or placing the highlighted item",
    install,
};

/// The space between the chance and the right edge of the title's line.
const RIGHT_MARGIN: f32 = 8.0;

// The game's words.
/// `ContainerMenu::pMe`, `ContainerMenu::pCurrItem`, the player.
const CONTAINER_MENU: u32 = 0x011d_93f8;
const CURRENT_ITEM: u32 = 0x011d_93fc;
const PLAYER: u32 = 0x011d_ea3c;
/// `ContainerMenu`: the reference (`+0x74`), the mode (`+0x80`, 2 pickpocketing), the current list
/// (`+0xf8`), the player's list (`+0x98`) and the reference's (`+0xc8`), the title tile (`+0x40`).
const CONTAINER_REF: u32 = 0x74;
const MODE: u32 = 0x80;
const PICKPOCKET: i32 = 2;
const CURRENT_LIST: u32 = 0xf8;
const ITEMS: u32 = 0x98;
const CONTAINER: u32 = 0xc8;
const TITLE: u32 = 0x40;
/// `Tile::pParent`.
const PARENT: u32 = 0x28;
/// A reference's actor slot, the target's pickpocket-free slot, the actor value owner, the
/// sneak-family actor value `0x2a`, `GetClampedActorValue` (`0066ef20`), the chance (`00643400`).
const IS_ACTOR_SLOT: u32 = 0x100;
const NO_CHECK_SLOT: u32 = 0x230;
const ACTOR_VALUES: u32 = 0xa4;
const PICKPOCKET_VALUE: u32 = 0x2a;
const GET_CLAMPED_ACTOR_VALUE: u32 = 0x0066_ef20;
const PICKPOCKET_CHANCE: u32 = 0x0064_3400;
/// An item's owner (`004bd740`), its stolen value (`ContainerMenu::GetStolenValue`, `0075e240`),
/// gold (`00481f10`), a form's weight (`0048ebc0`, with the player's `004d1360`).
const ITEM_OWNER: u32 = 0x004b_d740;
const GET_STOLEN_VALUE: u32 = 0x0075_e240;
const IS_GOLD: u32 = 0x0048_1f10;
const FORM_WEIGHT: u32 = 0x0048_ebc0;
const PLAYER_STATE: u32 = 0x004d_1360;
/// The quantity menu (`QuantityMenu::pMe`), the function it gives its count to, its value tile
/// (`+0x34`), `ContainerMenu::ItemSelectCallback`, a tile's float.
const QUANTITY_MENU: u32 = 0x011d_a618;
const QUANTITY_CALLBACK: u32 = 0x011d_a614;
const QUANTITY_VALUE_TILE: u32 = 0x34;
const ITEM_SELECT_CALLBACK: u32 = 0x0075_dc80;
const TILE_GET_FLOAT: u32 = 0x00a0_11b0;
/// The title's look, copied: font, depth, system color, red, green, blue, alpha, brightness; and
/// the y.
const LOOK: [i32; 8] = [0xfb9, 0xfad, 0xff4, 0xfb2, 0xfb3, 0xfb4, 0xfa9, 0xff5];
const TRAIT_Y: i32 = 0xfa2;

/// The tile and the menu and title parent it was made for.
#[derive(Clone, Copy, Debug, Default)]
struct Shown {
    menu: u32,
    parent: u32,
    tile: u32,
}

static SHOWN: Mutex<Option<Shown>> = Mutex::new(None);

fn shown() -> Option<Shown> {
    *SHOWN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn keep(shown: Option<Shown>) {
    *SHOWN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = shown;
}

/// Registers `DoIdle`.
pub fn install(e: &mut Engine) {
    e.register(DO_IDLE, |e, a| {
        container_menu_do_idle(e, Ptr::new(a[0]));
        update(e, a[0]);
        Ret::default()
    });
}

// The parts that need no game.

/// What `ItemSelectCallback` checks for the highlighted item: no check at all, or
/// `PickpocketCheck` with this value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    None,
    Value(i32),
}

/// `ItemSelectCallback`'s steps before `PickpocketCheck` (mode 2): the target is the player when
/// the player's list is current (`from_player`); placing on an actor always counts, the player's
/// own items otherwise never, and an item of the reference's list counts unless the player owns it
/// (`owned_by_player`). The value is the stolen value when taking (the reference's list current,
/// `taking`), else 1. Without an actor nothing is checked.
pub fn check(
    from_player: bool,
    taking: bool,
    actor: bool,
    owned_by_player: bool,
    stolen: i32,
) -> Check {
    let counts = if from_player { actor } else { !owned_by_player };
    if !counts || !actor {
        return Check::None;
    }
    Check::Value(if taking { stolen } else { 1 })
}

/// The success percentage of a `PickpocketCheck` chance: `roll < chance` for a roll of 0 to 99, both
/// unsigned.
pub fn success_percent(chance: u32) -> u32 {
    chance.min(100)
}

/// The text shown.
pub fn label(percent: u32) -> Vec<u8> {
    format!("Chance: {percent}%").into_bytes()
}

// The game.

/// The chance of the highlighted item in the open pickpocket menu `menu`, in percent, or `None`
/// when there is none to show.
fn chance(e: &mut Engine, menu: u32) -> Option<u32> {
    if e.mem.i32(menu + MODE) != PICKPOCKET {
        return None;
    }
    let reference = e.mem.u32(menu + CONTAINER_REF);
    if reference == 0 || !e.vcall(reference, IS_ACTOR_SLOT, &args![]).bool() {
        return None;
    }
    let actor = reference;
    let item = e.global::<u32>(CURRENT_ITEM);
    let list = e.mem.u32(menu + CURRENT_LIST);
    let from_player = list == menu + ITEMS;
    let taking = list == menu + CONTAINER;
    if item == 0 || !(from_player || taking) {
        return None;
    }
    let player = e.global::<u32>(PLAYER);
    let owned_by_player = !from_player && {
        let owner = e.call(ITEM_OWNER, &args![item]).u32();
        owner == e.mem.u32(player + 0x20)
    };
    let stolen = if taking {
        let count = take_count(e, menu, item, taking);
        e.call(GET_STOLEN_VALUE, &args![menu, item, count, 1u32])
            .i32()
    } else {
        0
    };
    let value = match check(from_player, taking, true, owned_by_player, stolen) {
        Check::None => return Some(100),
        Check::Value(value) => value,
    };
    // `PickpocketCheck`'s inputs.
    if value <= 0 || e.vcall(actor, NO_CHECK_SLOT, &args![]).bool() {
        return Some(100);
    }
    let target_value = e
        .vcall(actor + ACTOR_VALUES, 8, &args![PICKPOCKET_VALUE])
        .u32();
    let player_value = e
        .call(
            GET_CLAMPED_ACTOR_VALUE,
            &args![player + ACTOR_VALUES, PICKPOCKET_VALUE],
        )
        .u32();
    let amount = value as f32;
    let chance = e
        .call(
            PICKPOCKET_CHANCE,
            &args![player_value, target_value, amount],
        )
        .u32();
    Some(success_percent(chance))
}

/// The count a choice of `item` moves: the quantity menu's while it is open for this menu, the
/// whole stack for gold or a weightless item taken from them (`DoClick`'s rule), else one.
fn take_count(e: &mut Engine, _menu: u32, item: u32, taking: bool) -> i32 {
    let quantity = e.global::<u32>(QUANTITY_MENU);
    if quantity != 0 && e.global::<u32>(QUANTITY_CALLBACK) == ITEM_SELECT_CALLBACK {
        let tile = e.mem.u32(quantity + QUANTITY_VALUE_TILE);
        if tile != 0 {
            let value = e
                .call(TILE_GET_FLOAT, &args![tile, TRAIT_STRING as u32])
                .f64();
            if value >= 1.0 {
                return value as i32;
            }
        }
    }
    let stack = e.mem.i32(item + 4);
    let object = e.mem.u32(item + 8);
    if taking && stack > 1 && object != 0 {
        let gold = e.call(IS_GOLD, &args![object]).bool();
        let weightless = gold || {
            let player = e.global::<u32>(PLAYER);
            let flag = e.call(PLAYER_STATE, &args![player]).u8();
            e.call(FORM_WEIGHT, &args![object, u32::from(flag)]).f64() <= 0.0
        };
        if weightless {
            return stack;
        }
    }
    1
}

/// The tile for `menu`, made under its title's parent when there is none yet.
fn tile_for(e: &mut Engine, menu: u32, parent: u32) -> Option<u32> {
    if let Some(shown) = shown() {
        if shown.menu == menu && shown.parent == parent && is_child(e, parent, shown.tile) {
            return Some(shown.tile);
        }
    }
    const XML: &[u8] = b"<text name=\"nvrs_pickpocket_chance\"><x>0</x><y>0</y>\
        <visible>0</visible></text>\n";
    let root = read_xml(e, parent, XML, b"nv-rs pickpocket chance")?;
    let tile = if root != 0 {
        root
    } else {
        child(e, parent, "nvrs_pickpocket_chance")
    };
    keep(Some(Shown { menu, parent, tile }));
    Some(tile)
}

/// The chance shown or hidden for the container menu `menu` (module documentation).
fn update(e: &mut Engine, menu: u32) {
    if menu == 0 || e.global::<u32>(CONTAINER_MENU) != menu {
        return;
    }
    let percent = chance(e, menu);
    let title = e.mem.u32(menu + TITLE);
    let parent = if title == 0 {
        0
    } else {
        e.mem.u32(title + PARENT)
    };
    let Some(percent) = percent.filter(|_| parent != 0) else {
        if let Some(shown) = shown() {
            if shown.menu == menu && is_child(e, shown.parent, shown.tile) {
                set_visible(e, shown.tile, false);
            }
        }
        return;
    };
    let Some(tile) = tile_for(e, menu, parent) else {
        return;
    };
    for id in LOOK {
        if let Some(value) = trait_value(e, title, id) {
            set_value(e, tile, id, value);
        }
    }
    set_text(e, tile, TRAIT_STRING, &label(percent));
    // On the title's line, ending at the right edge of the title's parent, in the title's frame
    // (siblings: the same ancestors).
    let title_x = trait_value(e, title, TRAIT_X).unwrap_or(0.0);
    let title_left = fn_00a013d0(e, Ptr::new(title));
    let parent_left = fn_00a013d0(e, Ptr::new(parent));
    let parent_width = fn_00a011b0(e, Ptr::new(parent), TRAIT_WIDTH);
    let width = trait_value(e, tile, TRAIT_WIDTH).unwrap_or(0.0);
    let right = title_x + (parent_left + parent_width - title_left);
    set_value(e, tile, TRAIT_X, (right - RIGHT_MARGIN - width).round());
    if let Some(y) = trait_value(e, title, TRAIT_Y) {
        set_value(e, tile, TRAIT_Y, y);
    }
    set_visible(e, tile, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taking_checks_the_stolen_value_and_placing_checks_one() {
        // Taking an item they own: its stolen value.
        assert_eq!(check(false, true, true, false, 250), Check::Value(250));
        // Taking an item the player owns (one the player placed): no check.
        assert_eq!(check(false, true, true, true, 250), Check::None);
        // Placing one of the player's items on them: a value of 1.
        assert_eq!(check(true, false, true, false, 0), Check::Value(1));
        // Not an actor: nothing is checked.
        assert_eq!(check(false, true, false, false, 250), Check::None);
        assert_eq!(check(true, false, false, false, 0), Check::None);
    }

    #[test]
    fn the_percentage_is_the_chance_a_roll_of_0_to_99_is_below() {
        assert_eq!(success_percent(0), 0);
        assert_eq!(success_percent(75), 75);
        assert_eq!(success_percent(100), 100);
        assert_eq!(success_percent(140), 100);
        // A negative chance compared unsigned is above every roll: a sure success, as in the game.
        assert_eq!(success_percent(-5i32 as u32), 100);
        assert_eq!(label(75), b"Chance: 75%");
    }

    /// The pickpocket menu's chance for a taken item comes from `00643400` of the player's and the
    /// target's actor value `0x2a` and the stolen value as a float, with nothing rolled.
    #[test]
    fn the_chance_is_the_games_formula_of_the_checks_inputs() {
        use std::sync::{Arc, Mutex};
        let mut e = Engine::new();
        e.mem.map(0x1000, 0x4000);
        // The globals: the menu, the current item, the quantity menu, the player.
        e.mem.map(0x011d_9000, 0x6000);
        let menu = 0x1000;
        let actor = 0x2000;
        let player = 0x3000;
        let item = 0x3800;
        e.mem.set_u32(CONTAINER_MENU, menu);
        e.mem.set_u32(PLAYER, player);
        e.mem.set_u32(CURRENT_ITEM, item);
        e.mem.set_i32(menu + MODE, PICKPOCKET);
        e.mem.set_u32(menu + CONTAINER_REF, actor);
        e.mem.set_u32(menu + CURRENT_LIST, menu + CONTAINER);
        e.mem.set_i32(item + 4, 1);
        e.mem.set_u32(player + 0x20, 0x77);
        // Vtables: the actor answers it is one, and has no pickpocket-free state; its actor
        // values answer 40.
        let vtable = 0x2800;
        e.mem.set_u32(actor, vtable);
        e.mem.set_u32(vtable + IS_ACTOR_SLOT, 0x7e00_0001);
        e.mem.set_u32(vtable + NO_CHECK_SLOT, 0x7e00_0002);
        let values = 0x2900;
        e.mem.set_u32(actor + ACTOR_VALUES, values);
        e.mem.set_u32(values + 8, 0x7e00_0003);
        e.register(0x7e00_0001, |_, _| 1u32.into_ret());
        e.register(0x7e00_0002, |_, _| 0u32.into_ret());
        e.register(0x7e00_0003, |_, _| 40u32.into_ret());
        e.register(ITEM_OWNER, |_, _| 0x55u32.into_ret());
        e.register(GET_STOLEN_VALUE, |_, a| {
            assert_eq!(a[3], 1, "unowned items count when taking");
            (a[2] * 30).into_ret()
        });
        e.register(GET_CLAMPED_ACTOR_VALUE, |_, _| 60u32.into_ret());
        e.mem.set_u32(QUANTITY_MENU, 0);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        e.register_double(PICKPOCKET_CHANCE, move |_, a| {
            record
                .lock()
                .unwrap()
                .push((a[0], a[1], f32::from_bits(a[2])));
            63u32.into_ret()
        });
        assert_eq!(chance(&mut e, menu), Some(63));
        assert_eq!(*seen.lock().unwrap(), [(60, 40, 30.0)]);
        // Planting from the player's list: the value 1.
        e.mem.set_u32(menu + CURRENT_LIST, menu + ITEMS);
        assert_eq!(chance(&mut e, menu), Some(63));
        assert_eq!(seen.lock().unwrap()[1], (60, 40, 1.0));
        // Not the pickpocket mode: nothing to show.
        e.mem.set_i32(menu + MODE, 1);
        assert_eq!(chance(&mut e, menu), None);
    }
}
