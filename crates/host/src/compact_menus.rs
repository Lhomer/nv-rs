//! Compact menus (`NV_COMPACT_MENUS`), a row of the `fixes` preset ([`crate::fixes`],
//! docs/FIXES.md): **not 1:1 with FalloutNV.exe**, an nv-rs addition the player asked for (Patrick,
//! 2026-11-12: "the in-game UI for conversations, looting, bartering, etc. was made for console.
//! It's very big ... Make the text smaller and more compact so more information can fit in the
//! same amount of space"). In the `vanilla` preset, or with `bCompactMenus=0`, the menus are the
//! original's.
//!
//! **What changes.** The list-and-text menus of the conversation, looting and trading family
//! ([`COMPACT_CLASSES`]: `DialogMenu`, `ContainerMenu`, `BarterMenu`, `InventoryMenu`,
//! `RepairMenu`, `RepairServicesMenu`, `ItemModMenu`, `RecipeMenu`) write their text in the game's
//! medium font instead of its large ones ([`compact_font`]: fonts 1, 2, 4, 6 and 7 become font 3,
//! `Glow_Monofonto_Medium`), and every list of them shows more rows in the same height
//! ([`visible_items`]). Nothing else: the same menus, options, items, prices, stats, keys and
//! colors; only the size of the words and so the rows, their highlight and the wrapping. The HUD,
//! the Pip-Boy's other pages, the title and settings menus are not touched (their classes are not
//! in the set).
//!
//! **How.** The game reads a menu from its XML with `Tile::ReadFile` (`00a01b00`), which builds the
//! menu's tiles and hands the list-row templates to the menu (`Menu::xTemplates`, built later by
//! `Menu::RenderTemplate` for each row). The override runs the original reader unchanged, and then,
//! for a menu of a class in the set:
//! - every text tile of the new tree with a large font gets the medium one (setting `font` makes
//!   the game rebuild the text, `Tile::FinalPostParse`, so its height and what is computed from it,
//!   a row's height and its highlight, follow);
//! - every template's `font` element with a large font gets the medium one, so each row the menu
//!   makes from it later has it;
//! - every list container's `_number_of_visible_items` (the game sets a list's height to its first
//!   row's height times that number, `ListBox::Insert`) grows by the ratio of the two fonts' heights
//!   (`Font::fFontHeight`, with [`ROW_PADDING`] for the row's own spacing), so the list keeps the
//!   height it had and holds more rows.
//!
//! The menu files are the player's, as the game finds them (an archive or a UI mod's loose files);
//! nothing is written to them, and a menu file that sets other fonts or no list counts is left as
//! it is.

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::bsmenu::tile::{
    fn_00a011b0, tile_add_user_trait, tile_get_menu, tile_get_value, tile_read_file,
};

/// `Tile::ReadFile` (`00a01b00`).
pub const READ_FILE: u32 = 0x00a0_1b00;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "compact text",
    env: "NV_COMPACT_MENUS",
    addresses: &[READ_FILE],
    summary: "conversation, container, barter, inventory, repair and crafting menus write their text in the medium font, and their lists show more rows in the same space",
    install,
};

/// The menu classes made compact (each menu's `GetClass`): `DialogMenu` `0x3f1`, `ContainerMenu`
/// `0x3f0`, `BarterMenu` `0x41d`, `InventoryMenu` `0x3ea`, `RepairMenu` `0x40b`,
/// `RepairServicesMenu` `0x422`, `ItemModMenu` `0x425`, `RecipeMenu` `0x435`.
pub const COMPACT_CLASSES: [u32; 8] = [0x3f1, 0x3f0, 0x41d, 0x3ea, 0x40b, 0x422, 0x425, 0x435];
/// The game's fonts (`sFontFile_1` .. `_8`): 1 `Glow_Monofonto_Large`, 2 `Monofonto_Large`, 3
/// `Glow_Monofonto_Medium`, 4 `Monofonto_VeryLarge02_Dialogs2`, 6 `Glow_Monofonto_VL_dialogs`, 7
/// `Baked-in_Monofonto_Large`; the large ones and the medium one they become.
const LARGE_FONTS: [u32; 5] = [1, 2, 4, 6, 7];
pub const MEDIUM_FONT: u32 = 3;
/// The font a list row's height is reckoned from when the rows grow in number.
const LIST_FONT: u32 = 1;
/// A row's own spacing around its text, in the interface's units, for [`visible_items`]: a row is
/// counted as its text's height and this, so a list never grows past the height it had.
pub const ROW_PADDING: f32 = 6.0;

// The game's words.
/// `Menu::GetClass`, the menu's vtable slot.
const GET_CLASS_SLOT: u32 = 0x34;
/// `Menu::xTemplates` (`BSSimpleList`, its first node in the menu: the item, then the next node).
const MENU_TEMPLATES: u32 = 0x08;
/// `Tile::TileTemplate::xList` (the head node; a node's next at `+0`, its item at `+8`) and a
/// `Tile::TileTemplateItem`'s command, value and trait.
const TEMPLATE_ITEMS: u32 = 0x08;
const ITEM_VALUE: u32 = 0x04;
const ITEM_TRAIT: u32 = 0x10;
const TI_SIMPLE_TRAIT: i32 = 8;
/// The `font` trait, `Tile::SetFloat`, and the list count's user trait name
/// (`"_number_of_visible_items"`).
const TRAIT_FONT: i32 = 0xfb9;
const TILE_SET_FLOAT: u32 = 0x00a0_12d0;
const NUMBER_OF_VISIBLE_ITEMS_TEXT: u32 = 0x0106_f860;
/// The font manager (its word) and a font's height (`Font::fFontHeight`).
const FONT_MANAGER: u32 = 0x011f_33f8;
const FONT_HEIGHT: u32 = 0x2c;

/// Registers the reader.
pub fn install(e: &mut Engine) {
    e.register(READ_FILE, |e, a| {
        let root = tile_read_file(e, Ptr::new(a[0]), Ptr::new(a[1])).addr();
        compact(e, root);
        Ret {
            eax: root,
            ..Ret::default()
        }
    });
}

// The parts that need no game.

/// Whether a menu of class `class` is made compact.
pub fn is_compact(class: u32) -> bool {
    COMPACT_CLASSES.contains(&class)
}

/// The font a text of font `font` gets: the medium one for a large one, else none (it stays).
pub fn compact_font(font: f32) -> Option<f32> {
    let whole = font as u32;
    (whole as f32 == font && LARGE_FONTS.contains(&whole)).then_some(MEDIUM_FONT as f32)
}

/// A list's rows shown for `count` rows of text `from` high that become `to` high: as many as fit
/// in the height the list had, each row its text and [`ROW_PADDING`]; never fewer.
pub fn visible_items(count: f32, from: f32, to: f32) -> f32 {
    if count <= 0.0 || from <= 0.0 || to <= 0.0 || to >= from {
        return count;
    }
    (count * (from + ROW_PADDING) / (to + ROW_PADDING))
        .floor()
        .max(count)
}

// The game.

/// A font's height, if the font is loaded.
fn font_height(e: &Engine, font: u32) -> Option<f32> {
    let manager = e.global::<u32>(FONT_MANAGER);
    if manager == 0 || font == 0 {
        return None;
    }
    let object = e.mem.u32(manager + (font - 1) * 4);
    if object == 0 {
        return None;
    }
    let height = e.mem.f32(object + FONT_HEIGHT);
    (height > 0.0).then_some(height)
}

/// The menu read under `root` made compact (module documentation), when its class is in the set.
fn compact(e: &mut Engine, root: u32) {
    if root == 0 {
        return;
    }
    let menu = tile_get_menu(e, Ptr::new(root)).addr();
    if menu == 0 || !is_compact(e.vcall(menu, GET_CLASS_SLOT, &args![]).u32()) {
        return;
    }
    compact_templates(e, menu);
    let visible_trait = tile_add_user_trait(e, Ptr::new(NUMBER_OF_VISIBLE_ITEMS_TEXT), -1);
    let heights = (font_height(e, LIST_FONT), font_height(e, MEDIUM_FONT));
    // The new tree, depth first (`Tile::xChildren`: the head at `+4`; a node's next at `+0`, its
    // tile at `+8`).
    let mut stack = vec![root];
    while let Some(tile) = stack.pop() {
        compact_tile(e, tile, visible_trait, heights);
        let mut node = e.mem.u32(tile + 4);
        while node != 0 && stack.len() < 0x10000 {
            let child = e.mem.u32(node + 8);
            if child != 0 {
                stack.push(child);
            }
            node = e.mem.u32(node);
        }
    }
}

/// One tile of the new tree: its font, and its list count.
fn compact_tile(
    e: &mut Engine,
    tile: u32,
    visible_trait: i32,
    heights: (Option<f32>, Option<f32>),
) {
    let tile_ptr = Ptr::new(tile);
    if !tile_get_value(e, tile_ptr, TRAIT_FONT).is_null() {
        let font = fn_00a011b0(e, tile_ptr, TRAIT_FONT);
        if let Some(font) = compact_font(font) {
            e.call(TILE_SET_FLOAT, &args![tile, TRAIT_FONT as u32, font, 1u32]);
        }
    }
    if visible_trait > 0 && !tile_get_value(e, tile_ptr, visible_trait).is_null() {
        if let (Some(from), Some(to)) = heights {
            let count = fn_00a011b0(e, tile_ptr, visible_trait);
            let more = visible_items(count, from, to);
            if more != count {
                e.call(
                    TILE_SET_FLOAT,
                    &args![tile, visible_trait as u32, more, 1u32],
                );
            }
        }
    }
}

/// The menu's templates: each `font` element with a large font gets the medium one (a medium font
/// is not large again, so a template seen twice is the same).
fn compact_templates(e: &mut Engine, menu: u32) {
    let mut node = menu + MENU_TEMPLATES;
    let mut templates = 0;
    while node != 0 && templates < 4096 {
        let template = e.mem.u32(node);
        if template != 0 {
            let mut item_node = e.mem.u32(template + TEMPLATE_ITEMS);
            let mut items = 0;
            while item_node != 0 && items < 0x10000 {
                let item = e.mem.u32(item_node + 8);
                if item != 0
                    && e.mem.i32(item) == TI_SIMPLE_TRAIT
                    && e.mem.i32(item + ITEM_TRAIT) == TRAIT_FONT
                {
                    if let Some(font) = compact_font(e.mem.f32(item + ITEM_VALUE)) {
                        e.mem.set_f32(item + ITEM_VALUE, font);
                    }
                }
                item_node = e.mem.u32(item_node);
                items += 1;
            }
        }
        node = e.mem.u32(node + 4);
        templates += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_large_fonts_become_the_medium_one_and_the_rest_stay() {
        for large in [1.0, 2.0, 4.0, 6.0, 7.0] {
            assert_eq!(compact_font(large), Some(3.0), "{large}");
        }
        for other in [0.0, 3.0, 5.0, 8.0, 1.5] {
            assert_eq!(compact_font(other), None, "{other}");
        }
        // Once medium, never changed again.
        assert_eq!(compact_font(compact_font(1.0).unwrap()), None);
    }

    #[test]
    fn a_list_shows_more_rows_in_no_more_height() {
        // Rows of 30 high text that become 22: 7 rows become 9, which fit in what 7 took.
        let more = visible_items(7.0, 30.0, 22.0);
        assert_eq!(more, 9.0);
        assert!(more * (22.0 + ROW_PADDING) <= 7.0 * (30.0 + ROW_PADDING));
        // No list count, no change in size, or fonts unknown: as it was.
        assert_eq!(visible_items(0.0, 30.0, 22.0), 0.0);
        assert_eq!(visible_items(7.0, 22.0, 22.0), 7.0);
        assert_eq!(visible_items(7.0, 0.0, 22.0), 7.0);
        // Never fewer.
        assert_eq!(visible_items(3.0, 23.0, 22.0), 3.0);
    }

    #[test]
    fn the_conversation_looting_and_trading_menus_are_compact_and_the_hud_is_not() {
        for class in [0x3f1, 0x3f0, 0x41d, 0x3ea] {
            assert!(is_compact(class), "{class:#x}");
        }
        // The HUD, the start menu, the Pip-Boy's stats and map pages, the level-up menu.
        for class in [0x3ec, 0x3f5, 0x3eb, 0x3ff, 0x403] {
            assert!(!is_compact(class), "{class:#x}");
        }
    }

    /// A menu whose one template holds `<font>1</font>`, `<font>3</font>` and `<x>1</x>`: only the
    /// large font changes.
    #[test]
    fn a_templates_large_font_becomes_medium() {
        let mut e = Engine::new();
        e.mem.map(0x1000, 0x1000);
        let menu = 0x1000;
        let template = 0x1100;
        e.mem.set_u32(menu + MENU_TEMPLATES, template);
        e.mem.set_u32(menu + MENU_TEMPLATES + 4, 0);
        // Three nodes at 0x1200, 0x1210, 0x1220, items at 0x1300, 0x1320, 0x1340.
        e.mem.set_u32(template + TEMPLATE_ITEMS, 0x1200);
        let items = [
            (0x1300, TRAIT_FONT, 1.0),
            (0x1320, TRAIT_FONT, 3.0),
            (0x1340, 0xfa1, 1.0),
        ];
        for (index, (item, trait_id, value)) in items.iter().enumerate() {
            let node = 0x1200 + index as u32 * 0x10;
            let next = if index + 1 < items.len() {
                node + 0x10
            } else {
                0
            };
            e.mem.set_u32(node, next);
            e.mem.set_u32(node + 8, *item);
            e.mem.set_i32(*item, TI_SIMPLE_TRAIT);
            e.mem.set_f32(item + ITEM_VALUE, *value);
            e.mem.set_i32(item + ITEM_TRAIT, *trait_id);
        }
        compact_templates(&mut e, menu);
        assert_eq!(e.mem.f32(0x1300 + ITEM_VALUE), 3.0);
        assert_eq!(e.mem.f32(0x1320 + ITEM_VALUE), 3.0);
        assert_eq!(e.mem.f32(0x1340 + ITEM_VALUE), 1.0, "not a font");
    }
}
