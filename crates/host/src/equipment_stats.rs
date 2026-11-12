//! Equipment stats (`NV_EQUIPMENT_STATS`), a row of the `fixes` preset ([`crate::fixes`],
//! docs/FIXES.md): **not 1:1 with FalloutNV.exe**, an nv-rs addition the player asked for
//! (Patrick, 2026-11-12: every weapon and armour stat shown at once, and the equipped item of the
//! same slot shown beside the one highlighted). In the `vanilla` preset, or with
//! `bEquipmentStats=0`, the stats are the original's.
//!
//! **The item stats display.** The inventory, container (looting, pickpocketing, companions) and
//! barter menus show the highlighted item's stats with `Interface::PopulateItemStatsDisplay`
//! (`00707e30`): the display tile's first 13 children are its cells (0 DR, 1 DPS, 2 WG, 3 VAL,
//! 4 CND, 5 ammo or armour type, 6 effects, 7..9 mods, 10 STR, 11 DAM, 12 DT), each with a title
//! and a value (the user traits `0106ed20` and `0106ed18`). Every frame the menu's `DoIdle` hands
//! two of them to `00719790`, which fades one in while the other fades out, in the same place:
//!
//! - DAM and DPS of a weapon (cells 11 and 1, the menus' tiles `+0x60` and `+0x64`);
//! - DR and DT of armour (cells 0 and 12, tiles `+0x6c` and `+0x70`), when it has both;
//! - in the Pip-Boy inventory's status line, the player's DR and DT (tiles `+0x30` and `+0x7c`,
//!   with a DR above 0).
//!
//! Those are the cycling pairs of the game; the third tile `00719790` takes (STR, cell 10, which
//! blinks while the player's Strength is below the weapon's) is a warning, not a pair, and keeps
//! blinking. With this row a pair is never faded ([`set_item_card`]): both tiles stay opaque. A
//! pair of cells gets a line of its own: the second cell (DPS, DT) moves to just above the first
//! (DAM, DR), where the STR cell's row leaves room on the left; it goes back to its place when the
//! next item has only one of the two ([`populate`]). The status line's pair has no room for a
//! second box, so its second box (DT) shows both, "DT/DR 15/20" ([`combine`]), and the first is
//! hidden.
//!
//! **The equipped item beside it.** When the display is filled for a weapon or a piece of armour,
//! the player's equipped items that the highlighted one would replace ([`replaces`]) are found:
//! for a weapon the equipped weapon (one weapon slot); for armour and clothing each worn piece
//! whose biped slots (`TESBipedModelForm`'s part mask, the word at `+4` of the form's biped part,
//! `00480db0`) share a slot with it, as equipping it takes off exactly those (head with head, body
//! with body, one slot at a time). For each (at most two) the display is filled with that item by
//! the game's own `PopulateItemStatsDisplay` and its name, DAM and DPS or DR and DT are read from
//! the cells (so the numbers are the game's, condition, mods and perks included); then the display
//! is filled with the highlighted item again, before anything is drawn, so its stats are never
//! overwritten. The equipped items show under the stats, one line each ("Equipped: Leather Armor
//! DT 6"), in a text tile of nv-rs's own made a child of the display (it hides with it). Nothing
//! shows for an empty slot, for an item that is not a weapon or armour, or for the equipped item
//! itself.

use crate::fixes::Fix;
use crate::menu_tiles::{child, is_child, read_xml, set_text, set_value, set_visible, trait_value};
use engine::prelude::*;
use engine::units::fallout_interface::interface_p2::interface_populate_item_stats_display;
use engine::units::fallout_interface::interfacemanager_p2::fn_00719790;
use std::sync::Mutex;

/// `Interface::PopulateItemStatsDisplay` (`00707e30`) and the stats fade (`00719790`).
pub const POPULATE: u32 = 0x0070_7e30;
pub const FADE: u32 = 0x0071_9790;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "equipment stats",
    env: "NV_EQUIPMENT_STATS",
    addresses: &[POPULATE, FADE],
    summary: "every weapon and armour stat shows at once (DAM and DPS, DR and DT), and the equipped item of the same slot shows under the highlighted one",
    install,
};

/// The cells of the display (its first 13 children): DR, DPS, STR, DAM, DT.
const CELLS: usize = 13;
const CELL_DR: usize = 0;
const CELL_DPS: usize = 1;
const CELL_DAM: usize = 11;
const CELL_DT: usize = 12;
/// The most equipped items shown under the stats.
pub const MOST_EQUIPPED: usize = 2;
/// The space between the stats and the equipped lines.
const GAP: f32 = 4.0;

// The game's words.
const PLAYER: u32 = 0x011d_ea3c;
/// Traits.
const TRAIT_Y: i32 = 0xfa2;
const TRAIT_X: i32 = 0xfa1;
const TRAIT_ALPHA: i32 = 0xfa9;
const TRAIT_VISIBLE: i32 = 0xfa3;
const TRAIT_LOCUS: i32 = 0xfa8;
const TRAIT_HEIGHT: i32 = 0xfb0;
const TRAIT_WIDTH: i32 = 0xfb1;
const TRAIT_WRAP_WIDTH: i32 = 0xfba;
/// The cells' title and value trait names (`"_title"`-like user traits, as the menus use them).
const TITLE_NAME: u32 = 0x0106_ed20;
const VALUE_NAME: u32 = 0x0106_ed18;
const TILE_GET_STRING: u32 = 0x00a0_11f0;
const TILE_SET_FLOAT: u32 = 0x00a0_12d0;
/// `Tile::TextToTrait` (`00a01860`).
const TEXT_TO_TRAIT: u32 = 0x00a0_1860;
/// `Tile::pParent`.
const PARENT: u32 = 0x28;
/// Items: the walk (`00575510`, `004ca330`, `004bf630`), an item's name (`004be2d0`), whether it
/// is worn (`ItemChange::GetWorn`, `004bddd0`), its delete (`004459e0`), a form's type
/// (`00401170`) and biped part (`TESBipedModelForm::GetFormAsBipedModel`, `00480db0`).
const INVENTORY_START: u32 = 0x0057_5510;
const NEXT_ITEM: u32 = 0x004c_a330;
const ITERATOR_DELETE: u32 = 0x004b_f630;
const ITEM_FULL_NAME: u32 = 0x004b_e2d0;
const ITEM_GET_WORN: u32 = 0x004b_ddd0;
const ITEM_DELETE: u32 = 0x0044_59e0;
const FORM_TYPE: u32 = 0x0040_1170;
const BIPED_PART: u32 = 0x0048_0db0;
const WEAPON: u32 = 0x28;
const ARMOR: u32 = 0x18;
const ARMOR_ADDON: u32 = 0x1a;

/// Registers the display's filler and the fade.
pub fn install(e: &mut Engine) {
    e.register(POPULATE, |e, a| {
        populate(e, a[0], a[1]);
        Ret::default()
    });
    e.register(FADE, |e, a| {
        set_item_card(e, a[0], a[1], a[2], a[3], a[4] as u8);
        Ret::default()
    });
}

// The parts that need no game.

/// What the highlighted item is to the equipment: a weapon, or apparel with its biped slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Weapon,
    Apparel(u32),
    Other,
}

/// Whether an equipped item of kind `worn` is what equipping `wanted` replaces: the weapon for a
/// weapon (one weapon slot), a piece of apparel sharing a biped slot for apparel.
pub fn replaces(wanted: Kind, worn: Kind) -> bool {
    match (wanted, worn) {
        (Kind::Weapon, Kind::Weapon) => true,
        (Kind::Apparel(a), Kind::Apparel(b)) => a & b != 0,
        _ => false,
    }
}

/// One equipped line: "Equipped: name  TITLE value  TITLE value".
pub fn equipped_line(name: &[u8], stats: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut line = b"Equipped: ".to_vec();
    line.extend_from_slice(name);
    for (title, value) in stats {
        line.extend_from_slice(b"  ");
        line.extend_from_slice(title);
        line.push(b' ');
        line.extend_from_slice(value);
    }
    line
}

/// The status line's pair in one box: the box's own title and value, then the other's, each
/// "a/b".
pub fn combine(title: &[u8], value: &[u8], other_title: &[u8], other_value: &[u8]) -> [Vec<u8>; 2] {
    let join = |a: &[u8], b: &[u8]| {
        let mut joined = a.to_vec();
        joined.push(b'/');
        joined.extend_from_slice(b);
        joined
    };
    [join(title, other_title), join(value, other_value)]
}

/// Where a cell moved above its partner goes: its partner's y less its own height.
pub fn stacked_y(partner_y: f32, own_height: f32) -> f32 {
    partner_y - own_height
}

// The game.

/// A moved cell and the partner it sits above; and the status box shown combined, with what the
/// game wrote into it and what was shown.
#[derive(Default)]
struct State {
    moved: Vec<(u32, u32)>,
    combined: Option<Combined>,
    /// The equipped lines' tiles, by display.
    lines: Vec<(u32, u32)>,
}

struct Combined {
    tile: u32,
    raw: [Vec<u8>; 2],
    shown: [Vec<u8>; 2],
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(State::default))
}

/// A string trait of `tile`, or nothing.
fn text_of(e: &mut Engine, tile: u32, trait_id: i32) -> Vec<u8> {
    if tile == 0 {
        return Vec::new();
    }
    let text = e.call(TILE_GET_STRING, &args![tile, trait_id as u32]).u32();
    if text == 0 {
        Vec::new()
    } else {
        e.mem.cstr(text)
    }
}

/// The title and value trait ids.
fn title_value_traits(e: &mut Engine) -> (i32, i32) {
    (
        e.call(TEXT_TO_TRAIT, &args![TITLE_NAME]).i32(),
        e.call(TEXT_TO_TRAIT, &args![VALUE_NAME]).i32(),
    )
}

fn parent_of(e: &Engine, tile: u32) -> u32 {
    if tile == 0 {
        0
    } else {
        e.mem.u32(tile + PARENT)
    }
}

/// The display's first 13 children (`Tile::xChildren`: the head at `+4`; a node's next at `+0`, its
/// tile at `+8`).
fn cells(e: &Engine, display: u32) -> [u32; CELLS] {
    let mut cells = [0u32; CELLS];
    let mut node = e.mem.u32(display + 4);
    let mut index = 0;
    while node != 0 && index < CELLS {
        cells[index] = e.mem.u32(node + 8);
        node = e.mem.u32(node);
        index += 1;
    }
    cells
}

/// `00719790` without fading a pair (module documentation): the translation runs for the third
/// tile and the level's clock alone; a pair of the display's cells is shown both, the second above
/// the first; the status line's pair is shown combined in its second box.
fn set_item_card(e: &mut Engine, first: u32, second: u32, third: u32, source: u32, which: u8) {
    if first == 0 || second == 0 {
        restore_combined(e, second);
        fn_00719790(
            e,
            Ptr::new(first),
            Ptr::new(second),
            Ptr::new(third),
            Ptr::new(source),
            which,
        );
        return;
    }
    fn_00719790(
        e,
        Ptr::NULL,
        Ptr::NULL,
        Ptr::new(third),
        Ptr::new(source),
        which,
    );
    if cell_pair(e, first, second) {
        set_alpha(e, first, 255.0);
        set_alpha(e, second, 255.0);
        let partner_y = trait_value(e, first, TRAIT_Y).unwrap_or(0.0);
        let height = trait_value(e, second, TRAIT_HEIGHT).unwrap_or(0.0);
        set_value(e, second, TRAIT_Y, stacked_y(partner_y, height));
        with_state(|s| {
            if !s.moved.iter().any(|&(tile, _)| tile == second) {
                s.moved.push((second, first));
                if s.moved.len() > 16 {
                    s.moved.remove(0);
                }
            }
        });
    } else {
        show_combined(e, first, second);
    }
}

/// A tile's alpha, written each frame as `00719790` writes it (`Tile::SetFloat`, flag 1).
fn set_alpha(e: &mut Engine, tile: u32, alpha: f32) {
    e.call(
        TILE_SET_FLOAT,
        &args![tile, TRAIT_ALPHA as u32, alpha, 1u32],
    );
}

/// Whether `first` and `second` are one of the display's two pairs: siblings at cells 11 and 1
/// (DAM and DPS) or 0 and 12 (DR and DT) of their parent's children. The status line's boxes
/// are not.
fn cell_pair(e: &Engine, first: u32, second: u32) -> bool {
    let parent = parent_of(e, first);
    if parent == 0 || parent != parent_of(e, second) {
        return false;
    }
    let cells = cells(e, parent);
    let index = |tile: u32| cells.iter().position(|&cell| cell == tile);
    is_cell_pair(index(first), index(second))
}

/// The cell indices of a pair the display fades: DAM and DPS, DR and DT, either way round.
pub fn is_cell_pair(first: Option<usize>, second: Option<usize>) -> bool {
    matches!(
        (first, second),
        (Some(CELL_DAM), Some(CELL_DPS))
            | (Some(CELL_DPS), Some(CELL_DAM))
            | (Some(CELL_DR), Some(CELL_DT))
            | (Some(CELL_DT), Some(CELL_DR))
    )
}

/// The status line's pair in its second box (module documentation).
fn show_combined(e: &mut Engine, first: u32, second: u32) {
    let (title_trait, value_trait) = title_value_traits(e);
    let current = [
        text_of(e, second, title_trait),
        text_of(e, second, value_trait),
    ];
    let raw = with_state(|s| match &s.combined {
        Some(c) if c.tile == second && c.shown == current => c.raw.clone(),
        _ => current.clone(),
    });
    let other_title = text_of(e, first, title_trait);
    let other_value = text_of(e, first, value_trait);
    let shown = combine(&raw[0], &raw[1], &other_title, &other_value);
    set_text(e, second, title_trait, &shown[0]);
    set_text(e, second, value_trait, &shown[1]);
    set_alpha(e, second, 255.0);
    set_alpha(e, first, 0.0);
    with_state(|s| {
        s.combined = Some(Combined {
            tile: second,
            raw,
            shown,
        })
    });
}

/// The status box back to what the game wrote, when its pair is over.
fn restore_combined(e: &mut Engine, second: u32) {
    let combined = with_state(|s| match &s.combined {
        Some(c) if c.tile == second && second != 0 => s.combined.take(),
        _ => None,
    });
    let Some(combined) = combined else {
        return;
    };
    let (title_trait, value_trait) = title_value_traits(e);
    let current = [
        text_of(e, second, title_trait),
        text_of(e, second, value_trait),
    ];
    if current == combined.shown {
        set_text(e, second, title_trait, &combined.raw[0]);
        set_text(e, second, value_trait, &combined.raw[1]);
    }
}

/// What an item is to the equipment.
fn kind_of(e: &mut Engine, item: u32) -> Kind {
    let form = e.mem.u32(item + 8);
    if form == 0 {
        return Kind::Other;
    }
    match e.call(FORM_TYPE, &args![form]).u32() & 0xff {
        WEAPON => Kind::Weapon,
        ARMOR | ARMOR_ADDON => {
            let part = e.call(BIPED_PART, &args![form]).u32();
            if part == 0 {
                Kind::Other
            } else {
                Kind::Apparel(e.mem.u32(part + 4))
            }
        }
        _ => Kind::Other,
    }
}

/// The player's worn items that `wanted` replaces, as the walk's copies (deleted by the caller,
/// `004459e0`), at most [`MOST_EQUIPPED`].
fn equipped_for(e: &mut Engine, wanted: Kind) -> Vec<u32> {
    let player = e.global::<u32>(PLAYER);
    let mut found = Vec::new();
    if player == 0 {
        return found;
    }
    e.with_stack(8, |e, slots| {
        let iterator_slot = slots.addr();
        let changes_slot = slots.addr() + 4;
        e.mem.set_u32(iterator_slot, 0);
        e.mem.set_u32(changes_slot, 0);
        e.call(
            INVENTORY_START,
            &args![player, 0u32, iterator_slot, changes_slot],
        );
        let iterator = e.mem.u32(iterator_slot);
        let changes = e.mem.u32(changes_slot);
        if iterator != 0 && changes != 0 {
            loop {
                let item = e.call(NEXT_ITEM, &args![changes, iterator]).u32();
                if item == 0 {
                    break;
                }
                let keep = found.len() < MOST_EQUIPPED
                    && e.call(ITEM_GET_WORN, &args![item, 0u32]).bool()
                    && replaces(wanted, kind_of(e, item));
                if keep {
                    found.push(item);
                } else {
                    e.call(ITEM_DELETE, &args![item, 1u32]);
                }
            }
        }
        if iterator != 0 {
            e.call(ITERATOR_DELETE, &args![iterator, 1u32]);
        }
    });
    found
}

/// The title and value of the shown cells of `which`.
fn cell_stats(e: &mut Engine, display: u32, which: &[usize]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let (title_trait, value_trait) = title_value_traits(e);
    let cells = cells(e, display);
    let mut stats = Vec::new();
    for &index in which {
        let cell = cells[index];
        if cell == 0 || trait_value(e, cell, TRAIT_VISIBLE) == Some(0.0) {
            continue;
        }
        let value = text_of(e, cell, value_trait);
        if value.is_empty() {
            continue;
        }
        stats.push((text_of(e, cell, title_trait), value));
    }
    stats
}

/// `PopulateItemStatsDisplay` with the moved cells put back, the equipped items' lines read first
/// and shown after (module documentation).
fn populate(e: &mut Engine, item: u32, display: u32) {
    if display != 0 {
        restore_moved(e, display);
    }
    let mut lines = Vec::new();
    if item != 0 && display != 0 {
        let wanted = kind_of(e, item);
        if wanted != Kind::Other && !e.call(ITEM_GET_WORN, &args![item, 0u32]).bool() {
            let which: &[usize] = if wanted == Kind::Weapon {
                &[CELL_DAM, CELL_DPS]
            } else {
                &[CELL_DR, CELL_DT]
            };
            let equipped = equipped_for(e, wanted);
            for &worn in &equipped {
                interface_populate_item_stats_display(e, worn, display);
                let stats = cell_stats(e, display, which);
                let name = e.call(ITEM_FULL_NAME, &args![worn]).u32();
                let name = if name == 0 {
                    Vec::new()
                } else {
                    e.mem.cstr(name)
                };
                lines.push(equipped_line(&name, &stats));
            }
            for worn in equipped {
                e.call(ITEM_DELETE, &args![worn, 1u32]);
            }
        }
    }
    // The highlighted item last: its stats are what stays.
    interface_populate_item_stats_display(e, item, display);
    if display != 0 {
        show_lines(e, display, &lines);
    }
}

/// The cells moved above their partners put back on them, for `display`.
fn restore_moved(e: &mut Engine, display: u32) {
    let moved = with_state(|s| {
        let (here, rest): (Vec<_>, Vec<_>) = s
            .moved
            .iter()
            .partition(|&&(tile, _)| e.mem.u32(tile + PARENT) == display);
        s.moved = rest;
        here
    });
    for (tile, partner) in moved {
        if is_child(e, display, tile) && is_child(e, display, partner) {
            if let Some(y) = trait_value(e, partner, TRAIT_Y) {
                set_value(e, tile, TRAIT_Y, y);
            }
        }
    }
}

/// The equipped lines under the display, in nv-rs's text tile (a child of the display).
fn show_lines(e: &mut Engine, display: u32, lines: &[Vec<u8>]) {
    let known = with_state(|s| {
        s.lines
            .iter()
            .find(|&&(d, _)| d == display)
            .map(|&(_, tile)| tile)
    });
    let tile = match known {
        Some(tile) if is_child(e, display, tile) => tile,
        _ => {
            if lines.is_empty() {
                return;
            }
            const XML: &[u8] = b"<text name=\"nvrs_equipped\"><x>0</x><y>0</y><font>3</font>\
                <alpha>255</alpha><visible>0</visible></text>\n";
            let Some(root) = read_xml(e, display, XML, b"nv-rs equipped item") else {
                return;
            };
            let tile = if root != 0 {
                root
            } else {
                child(e, display, "nvrs_equipped")
            };
            with_state(|s| remember(s, display, tile));
            tile
        }
    };
    if tile == 0 {
        return;
    }
    if lines.is_empty() {
        set_visible(e, tile, false);
        return;
    }
    let mut text = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            text.push(b'\n');
        }
        text.extend_from_slice(line);
    }
    set_text(e, tile, crate::menu_tiles::TRAIT_STRING, &text);
    let height = trait_value(e, display, TRAIT_HEIGHT).unwrap_or(0.0);
    let width = trait_value(e, display, TRAIT_WIDTH).unwrap_or(0.0);
    let locus = trait_value(e, display, TRAIT_LOCUS).unwrap_or(0.0) != 0.0;
    let (x, y) = if locus {
        (0.0, height + GAP)
    } else {
        let x = trait_value(e, display, TRAIT_X).unwrap_or(0.0);
        let y = trait_value(e, display, TRAIT_Y).unwrap_or(0.0);
        (x, y + height + GAP)
    };
    set_value(e, tile, TRAIT_X, x.round());
    set_value(e, tile, TRAIT_Y, y.round());
    if width > 0.0 {
        set_value(e, tile, TRAIT_WRAP_WIDTH, width.round());
    }
    set_visible(e, tile, true);
}

fn remember(s: &mut State, display: u32, tile: u32) {
    match s.lines.iter_mut().find(|(d, _)| *d == display) {
        Some(entry) => entry.1 = tile,
        None => {
            s.lines.push((display, tile));
            if s.lines.len() > 16 {
                s.lines.remove(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weapon_replaces_the_weapon_and_apparel_the_pieces_sharing_its_slots() {
        const HEAD: u32 = 1 << 0;
        const HAIR: u32 = 1 << 1;
        const BODY: u32 = 1 << 2;
        const HANDS: u32 = 1 << 3;
        assert!(replaces(Kind::Weapon, Kind::Weapon));
        assert!(!replaces(Kind::Weapon, Kind::Apparel(BODY)));
        assert!(!replaces(Kind::Apparel(BODY), Kind::Weapon));
        // Headgear against headgear, body against body, never across.
        assert!(replaces(Kind::Apparel(HEAD | HAIR), Kind::Apparel(HEAD)));
        assert!(!replaces(Kind::Apparel(HEAD), Kind::Apparel(BODY)));
        assert!(!replaces(Kind::Apparel(BODY), Kind::Apparel(HANDS)));
        // A suit covering body and hands replaces gloves and armour both.
        assert!(replaces(Kind::Apparel(BODY | HANDS), Kind::Apparel(HANDS)));
        assert!(replaces(Kind::Apparel(BODY | HANDS), Kind::Apparel(BODY)));
        // No slots, or not equipment: nothing.
        assert!(!replaces(Kind::Apparel(0), Kind::Apparel(BODY)));
        assert!(!replaces(Kind::Other, Kind::Other));
    }

    #[test]
    fn the_equipped_line_names_the_item_and_its_stats() {
        let stats = [
            (b"DAM".to_vec(), b"18".to_vec()),
            (b"DPS".to_vec(), b"36".to_vec()),
        ];
        assert_eq!(
            equipped_line(b"Varmint Rifle", &stats),
            b"Equipped: Varmint Rifle  DAM 18  DPS 36"
        );
        assert_eq!(
            equipped_line(b"Leather Armor", &[(b"DT".to_vec(), b"6".to_vec())]),
            b"Equipped: Leather Armor  DT 6"
        );
    }

    #[test]
    fn the_status_lines_pair_shows_in_one_box() {
        let [title, value] = combine(b"DT", b"15", b"DR", b"20");
        assert_eq!(title, b"DT/DR");
        assert_eq!(value, b"15/20");
    }

    #[test]
    fn a_cell_moves_to_a_line_of_its_own_above_its_partner() {
        // DAM at y 40, DPS 24 high: DPS sits at 16, just above, not over it.
        assert_eq!(stacked_y(40.0, 24.0), 16.0);
        assert!(stacked_y(40.0, 24.0) + 24.0 <= 40.0);
    }

    /// Tiles whose strings live in a map: `Tile::GetString` and `SetString` read and write it,
    /// `SetFloat` is logged.
    type Strings = std::rc::Rc<std::cell::RefCell<std::collections::HashMap<(u32, u32), Vec<u8>>>>;
    type Floats = std::rc::Rc<std::cell::RefCell<Vec<(u32, u32, f32)>>>;

    fn tile_engine() -> (Engine, Strings, Floats) {
        use std::rc::Rc;
        let mut e = Engine::new();
        e.mem.map(0x1000, 0x4000);
        let strings: Strings = Rc::default();
        let floats: Floats = Rc::default();
        // The tile lock's critical section (`EnterCriticalSection`, `LeaveCriticalSection`).
        e.register(0x00fd_f05c, |_, _| Ret::default());
        e.register(0x00fd_f100, |_, _| Ret::default());
        e.register(TEXT_TO_TRAIT, |_, a| {
            (if a[0] == TITLE_NAME {
                10_000u32
            } else {
                10_001
            })
            .into_ret()
        });
        let read = strings.clone();
        e.register_double(TILE_GET_STRING, move |e, a| {
            let Some(text) = read.borrow().get(&(a[0], a[1])).cloned() else {
                return 0u32.into_ret();
            };
            let at = e.mem.alloc(text.len() as u32 + 1);
            e.mem.set_cstr(at, &text);
            at.into_ret()
        });
        let write = strings.clone();
        e.register_double(0x00a0_1350, move |e, a| {
            write.borrow_mut().insert((a[0], a[1]), e.mem.cstr(a[2]));
            Ret::default()
        });
        let log = floats.clone();
        e.register_double(0x00a0_12d0, move |_, a| {
            log.borrow_mut().push((a[0], a[1], f32::from_bits(a[2])));
            Ret::default()
        });
        (e, strings, floats)
    }

    #[test]
    fn the_status_line_shows_dt_and_dr_in_one_box_and_gives_it_back() {
        let (mut e, strings, floats) = tile_engine();
        let (dr, dt) = (0x1100u32, 0x1200u32);
        let (title, value) = (10_000u32, 10_001u32);
        for (tile, t, v) in [(dr, "DR", "20"), (dt, "DT", "15")] {
            strings
                .borrow_mut()
                .insert((tile, title), t.as_bytes().to_vec());
            strings
                .borrow_mut()
                .insert((tile, value), v.as_bytes().to_vec());
        }
        show_combined(&mut e, dr, dt);
        assert_eq!(strings.borrow()[&(dt, title)], b"DT/DR");
        assert_eq!(strings.borrow()[&(dt, value)], b"15/20");
        // The DR box is hidden (it sits in the same place), the DT box shown.
        assert!(floats.borrow().contains(&(dr, TRAIT_ALPHA as u32, 0.0)));
        assert!(floats.borrow().contains(&(dt, TRAIT_ALPHA as u32, 255.0)));
        // Shown again: not "DT/DR/DR".
        show_combined(&mut e, dr, dt);
        assert_eq!(strings.borrow()[&(dt, title)], b"DT/DR");
        // The game writes a new DT: it is taken as the box's own.
        strings.borrow_mut().insert((dt, value), b"16".to_vec());
        strings.borrow_mut().insert((dt, title), b"DT".to_vec());
        show_combined(&mut e, dr, dt);
        assert_eq!(strings.borrow()[&(dt, value)], b"16/20");
        // DR gone to 0 (the pair ends): the box is the game's again.
        restore_combined(&mut e, dt);
        assert_eq!(strings.borrow()[&(dt, title)], b"DT");
        assert_eq!(strings.borrow()[&(dt, value)], b"16");
        with_state(|s| *s = State::default());
    }

    #[test]
    fn only_the_displays_two_pairs_count_as_cells() {
        assert!(is_cell_pair(Some(CELL_DAM), Some(CELL_DPS)));
        assert!(is_cell_pair(Some(CELL_DR), Some(CELL_DT)));
        assert!(is_cell_pair(Some(CELL_DT), Some(CELL_DR)));
        assert!(!is_cell_pair(Some(CELL_DAM), Some(CELL_DT)));
        assert!(!is_cell_pair(Some(2), Some(3)));
        assert!(!is_cell_pair(None, Some(CELL_DPS)));
        assert!(!is_cell_pair(None, None));
    }

    #[test]
    fn the_cells_are_the_displays_first_thirteen_children() {
        let mut e = Engine::new();
        e.mem.map(0x1000, 0x2000);
        let display = 0x1000;
        // 15 children: nodes at 0x1800 + 0x10 * i (next, _, tile).
        e.mem.set_u32(display + 4, 0x1800);
        for i in 0..15u32 {
            let node = 0x1800 + 0x10 * i;
            e.mem.set_u32(node, if i < 14 { node + 0x10 } else { 0 });
            e.mem.set_u32(node + 8, 0x2000 + i);
        }
        let found = cells(&e, display);
        assert_eq!(found[CELL_DR], 0x2000);
        assert_eq!(found[CELL_DPS], 0x2001);
        assert_eq!(found[CELL_DAM], 0x200b);
        assert_eq!(found[CELL_DT], 0x200c);
    }
}
