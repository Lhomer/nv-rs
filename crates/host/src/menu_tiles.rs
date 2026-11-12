//! nv-rs's own tiles in the game's menus: menu XML held by nv-rs, read by the game's own reader
//! into tiles under a tile of a menu, and the trait helpers that fill them. Quick loot's panel
//! ([`crate::quick_loot`]) and the pickpocket chance ([`crate::pickpocket_chance`]) are made so; no
//! menu file is read or written for them.

use engine::prelude::*;
use engine::units::bsmenu::tile::{
    fn_00a011b0, tile_build_and_name_tree, tile_connect_traits_to_tree, tile_get_child_by_name,
    tile_get_value, tile_parse_file, BuildStorage,
};

/// `Tile::ReadFile`'s other steps: `BuildStorage`'s destructor, the memory manager's free, the
/// interface's lock (the owner's word, its section at `+0x80`), `Tile::UpdateChildren`, the
/// reader's error flag and the thread's current-line word (13 while menu files are read).
const BUILD_STORAGE_DESTROY: u32 = 0x00a0_ad40;
const MEMORY_DEALLOCATE: u32 = 0x00aa_4060;
const MEMORY_MANAGER: u32 = 0x011f_6238;
const INTERFACE_LOCK_OWNER: u32 = 0x011f_4748;
const ENTER_CRITICAL_SECTION: u32 = 0x00fd_f05c;
const LEAVE_CRITICAL_SECTION: u32 = 0x00fd_f100;
const TILE_UPDATE_CHILDREN: u32 = 0x00a0_4620;
const XML_ERROR_FLAG: u32 = 0x011f_32c8;
const TLS_CURRENT_LINE: u32 = 0x2b4;
/// `Tile::GetString`, `Tile::SetFloat`, `Tile::SetString`.
const TILE_GET_STRING: u32 = 0x00a0_11f0;
const TILE_SET_FLOAT: u32 = 0x00a0_12d0;
const TILE_SET_STRING: u32 = 0x00a0_1350;
/// Traits.
pub const TRAIT_X: i32 = 0xfa1;
pub const TRAIT_VISIBLE: i32 = 0xfa3;
pub const TRAIT_WIDTH: i32 = 0xfb1;
pub const TRAIT_RED: i32 = 0xfb2;
pub const TRAIT_GREEN: i32 = 0xfb3;
pub const TRAIT_BLUE: i32 = 0xfb4;
pub const TRAIT_STRING: i32 = 0xfc4;
pub const TRAIT_BRIGHTNESS: i32 = 0xff5;
/// The `brightness` that makes a tile use its own `red`, `green` and `blue` ([`set_color`]).
pub const OWN_COLOR: f32 = -1.0;

/// Reads `xml` into tiles under `parent` by the steps of `Tile::ReadFile` (`00a01b00`) for a text
/// in memory: `Tile::ParseFile` with the buffer, `BuildAndNameTree`, `ConnectTraitsToTree`, the
/// storage deleted, the new tree updated under the interface's lock. `path` names it in the
/// reader's messages. Returns the first new tile, or `None` after a reader error.
pub fn read_xml(e: &mut Engine, parent: u32, xml: &[u8], path: &[u8]) -> Option<u32> {
    let line_word = e.tls() + TLS_CURRENT_LINE;
    let saved_line = e.mem.u32(line_word);
    let size = xml.len() as u32 + path.len() as u32 + 2;
    let root = e.with_stack(size, |e, buffer| {
        let text = buffer.addr();
        let path_text = text + xml.len() as u32 + 1;
        e.mem.set_cstr(text, xml);
        e.mem.set_cstr(path_text, path);
        e.mem.set_u32(line_word, 0xd);
        e.set_global(XML_ERROR_FLAG, 0u8);
        let storage = tile_parse_file(e, Ptr::new(path_text), Ptr::new(text), xml.len() as u32);
        if e.global::<u8>(XML_ERROR_FLAG) != 0 || storage.is_null() {
            return None;
        }
        let template = |e: &Engine| {
            let current = e.get(storage, BuildStorage::pCurrentTemplate);
            if current.is_null() {
                e.get(storage, BuildStorage::pTemplate)
            } else {
                current
            }
        };
        let tile = tile_build_and_name_tree(e, Ptr::new(parent), template(e));
        if e.global::<u8>(XML_ERROR_FLAG) != 0 || tile.is_null() {
            return None;
        }
        tile_connect_traits_to_tree(e, tile, template(e));
        if e.global::<u8>(XML_ERROR_FLAG) != 0 {
            return None;
        }
        e.call(BUILD_STORAGE_DESTROY, &args![storage]);
        e.call(MEMORY_DEALLOCATE, &args![MEMORY_MANAGER, storage]);
        let lock = e.global::<u32>(INTERFACE_LOCK_OWNER) + 0x80;
        e.call(ENTER_CRITICAL_SECTION, &args![lock]);
        e.call(TILE_UPDATE_CHILDREN, &args![tile, 0u32]);
        e.call(LEAVE_CRITICAL_SECTION, &args![lock]);
        Some(tile.addr())
    });
    e.set_global(XML_ERROR_FLAG, 0u8);
    e.mem.set_u32(line_word, saved_line);
    root
}

/// The tile named `name` under `root` (`Tile::GetChildByName`, depth first), or 0.
pub fn child(e: &mut Engine, root: u32, name: &str) -> u32 {
    e.with_stack(name.len() as u32 + 1, |e, buffer| {
        e.mem.set_cstr(buffer.addr(), name.as_bytes());
        tile_get_child_by_name(e, Ptr::new(root), buffer).addr()
    })
}

/// Whether `child` is one of `parent`'s children (`Tile::xChildren`, the list at `+4`: its head
/// first; a node's next at `+0`, its tile at `+8`).
pub fn is_child(e: &Engine, parent: u32, child: u32) -> bool {
    if parent == 0 || child == 0 {
        return false;
    }
    let mut node = e.mem.u32(parent + 4);
    let mut steps = 0;
    while node != 0 && steps < 4096 {
        if e.mem.u32(node + 8) == child {
            return true;
        }
        node = e.mem.u32(node);
        steps += 1;
    }
    false
}

/// A float trait of `tile`, `None` when the tile has no such trait.
pub fn trait_value(e: &mut Engine, tile: u32, id: i32) -> Option<f32> {
    if tile == 0 || tile_get_value(e, Ptr::new(tile), id).is_null() {
        None
    } else {
        Some(fn_00a011b0(e, Ptr::new(tile), id))
    }
}

/// Sets a float trait of `tile` when it differs (a change makes the tile update).
pub fn set_value(e: &mut Engine, tile: u32, id: i32, value: f32) {
    if tile != 0 && trait_value(e, tile, id) != Some(value) {
        e.call(TILE_SET_FLOAT, &args![tile, id as u32, value, 1u32]);
    }
}

/// Sets the string trait `id` of `tile` when it differs.
pub fn set_text(e: &mut Engine, tile: u32, id: i32, text: &[u8]) {
    if tile == 0 {
        return;
    }
    let current = e.call(TILE_GET_STRING, &args![tile, id as u32]).u32();
    if current != 0 && e.mem.cstr(current) == text {
        return;
    }
    e.with_stack(text.len() as u32 + 1, |e, buffer| {
        e.mem.set_cstr(buffer.addr(), text);
        e.call(TILE_SET_STRING, &args![tile, id as u32, buffer, 1u32]);
    });
}

/// A tile's color: its `red`, `green` and `blue` (0 to 255), which the game uses only while its
/// `brightness` is -1 (`Tile::UpdateTile`'s color step; any other brightness, a missing one
/// included, takes the system color of the nearest ancestor that has one, the HUD's color in
/// the HUD), so the brightness is made -1 too.
pub fn set_color(e: &mut Engine, tile: u32, color: [u8; 3]) {
    set_value(e, tile, TRAIT_BRIGHTNESS, OWN_COLOR);
    set_value(e, tile, TRAIT_RED, f32::from(color[0]));
    set_value(e, tile, TRAIT_GREEN, f32::from(color[1]));
    set_value(e, tile, TRAIT_BLUE, f32::from(color[2]));
}

/// Shows or hides a tile.
pub fn set_visible(e: &mut Engine, tile: u32, visible: bool) {
    set_value(e, tile, TRAIT_VISIBLE, if visible { 1.0 } else { 0.0 });
}

/// Places a text tile so that it ends at `right` (its width is its text's, from its last update).
pub fn place_from_right(e: &mut Engine, tile: u32, right: f32) {
    let width = trait_value(e, tile, TRAIT_WIDTH).unwrap_or(0.0);
    set_value(e, tile, TRAIT_X, (right - width).round());
}
