//! Equipment stats (`NV_EQUIPMENT_STATS`), a row of the `fixes` preset ([`crate::fixes`],
//! docs/FIXES.md): **not 1:1 with FalloutNV.exe**, an nv-rs addition the player asked for
//! (Patrick, 2026-11-12: every weapon and armour stat shown at once, and the equipped item of the
//! same slot shown beside the one highlighted). In the `vanilla` preset, or with
//! `bEquipmentStats=0`, the stats are the original's.
//!
//! **The item stats display.** The inventory, container (looting, pickpocketing, companions) and
//! barter menus show the highlighted item's stats with `Interface::PopulateItemStatsDisplay`
//! (`00707e30`): the display tile's 13 cells, its children in menu-file order (taken from the
//! list's tail; [`cells`]), are (0 DR, 1 DPS, 2 WG, 3 VAL,
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
//! Only the inventory's own status boxes (`InventoryMenu +0x30` and `+0x7c`) are ever combined, and
//! a combined box is built each time from its own text, with what was added before taken off
//! ([`own_text`]): it cannot grow. The first build (1a85202) read the cells from the head of the
//! children list, not from the tail as the game does, so the card's DAM and DPS were not
//! recognised as cells and were combined instead; with one remembered box for two pairs a frame,
//! each frame added once more ("DPS/DAM/DAM/DAM...", Patrick's screenshot), and the equipped
//! weapon's line read WG where DAM and DPS are. A pair that is neither the cells nor the status
//! boxes is left to the game's own fade.
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
use engine::units::bsmenu::tile::fn_00a01440;
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

/// The cells of the display (13 of its children, [`cells`]): DR, DPS, DAM, DT.
const CELLS: usize = 13;
const CELL_DR: usize = 0;
const CELL_DPS: usize = 1;
const CELL_DAM: usize = 11;
const CELL_DT: usize = 12;
/// The most equipped items shown under the stats.
pub const MOST_EQUIPPED: usize = 2;
/// The space between the stats and the equipped lines.
const GAP: f32 = 4.0;
/// A line of the lines' font (font 3) when the text has not been drawn yet, and the room kept
/// above the screen's bottom.
const LINE_HEIGHT: f32 = 24.0;
const SCREEN_MARGIN: f32 = 8.0;
/// `InterfaceManager::GetScreenHeight` (the interface's units).
const SCREEN_HEIGHT: u32 = 0x0071_5da0;

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
const TRAIT_DEPTH: i32 = 0xfad;
/// The cells' title and value trait names (`"_title"`-like user traits, as the menus use them).
const TITLE_NAME: u32 = 0x0106_ed20;
const VALUE_NAME: u32 = 0x0106_ed18;
const TILE_GET_STRING: u32 = 0x00a0_11f0;
const TILE_SET_FLOAT: u32 = 0x00a0_12d0;
/// `Tile::GetFloat` (`00a011b0`, 0.0 for a missing trait; every cell's visibility is set by
/// `PopulateItemStatsDisplay`).
const TILE_GET_FLOAT: u32 = 0x00a0_11b0;
/// `Tile::TextToTrait` (`00a01860`).
const TEXT_TO_TRAIT: u32 = 0x00a0_1860;
/// `Tile::pParent`, the children list's tail (`Tile::xChildren` at `+4`, `m_pkTail` `+4` in it),
/// and a list node's previous link.
const PARENT: u32 = 0x28;
const CHILDREN_TAIL: u32 = 0x08;
const NODE_PREVIOUS: u32 = 0x04;
/// `InventoryMenu::pMe` and its status line's DR and DT boxes.
const INVENTORY_MENU: u32 = 0x011d_9ea4;
const STATUS_DR: u32 = 0x30;
const STATUS_DT: u32 = 0x7c;
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

/// A moved cell and the partner it sits above; the status box shown combined and the suffixes
/// it was given (the title's and the value's, "/DR" and "/20"), which are taken off again before
/// anything is added; and the equipped lines' tiles, by display.
#[derive(Default)]
struct State {
    moved: Vec<(u32, u32)>,
    combined: Vec<(u32, [Vec<u8>; 2])>,
    lines: Vec<(u32, u32)>,
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

/// The display's 13 cells, as `PopulateItemStatsDisplay` takes them: from the tail of the children
/// list (`Tile::xChildren` at `+4`, its tail at `+8`) along each node's previous link (`+4`; the
/// item at `+8`), which is the order of the menu file (`Tile::SetParent` puts each new child at the
/// head). A child added later (the equipped lines' tile) is at the head, after the cells.
fn cells(e: &Engine, display: u32) -> [u32; CELLS] {
    let mut cells = [0u32; CELLS];
    let mut node = e.mem.u32(display + CHILDREN_TAIL);
    let mut index = 0;
    while node != 0 && index < CELLS {
        cells[index] = e.mem.u32(node + 8);
        node = e.mem.u32(node + NODE_PREVIOUS);
        index += 1;
    }
    cells
}

/// `00719790` without fading a pair (module documentation): the translation runs for the third
/// tile and the level's clock alone; a pair of the display's cells is shown both, the second above
/// the first; the status line's pair is shown combined in its second box.
fn set_item_card(e: &mut Engine, first: u32, second: u32, third: u32, source: u32, which: u8) {
    let pair = if first == 0 || second == 0 {
        Pair::None
    } else if cell_pair(e, first, second) {
        Pair::Cells
    } else if status_pair(e, first, second) {
        Pair::Status
    } else {
        Pair::Other
    };
    if matches!(pair, Pair::None | Pair::Other) {
        if second != 0 {
            restore_combined(e, second);
        }
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
    if pair == Pair::Cells {
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

/// What `00719790` was given: no pair, a pair of the display's cells, the status line's pair, or
/// a pair of neither (faded as the game fades it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pair {
    None,
    Cells,
    Status,
    Other,
}

/// Whether `first` and `second` are the inventory's status line DR and DT boxes (`InventoryMenu`
/// `+0x30` and `+0x7c`): the only pair shown combined.
fn status_pair(e: &Engine, first: u32, second: u32) -> bool {
    let menu = e.global::<u32>(INVENTORY_MENU);
    menu != 0 && first == e.mem.u32(menu + STATUS_DR) && second == e.mem.u32(menu + STATUS_DT)
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

/// The status line's pair in its second box (module documentation): its own title and value, as
/// the game wrote them (what an earlier call added taken off: [`own_text`]), then "/" and the
/// other's. As often as it runs, the box reads "DT/DR 15/20", never "DT/DR/DR".
fn show_combined(e: &mut Engine, first: u32, second: u32) {
    let (title_trait, value_trait) = title_value_traits(e);
    let added = with_state(|s| {
        s.combined
            .iter()
            .find(|(tile, _)| *tile == second)
            .map(|(_, added)| added.clone())
    });
    let other = [
        text_of(e, first, title_trait),
        text_of(e, first, value_trait),
    ];
    let suffix = |text: &[u8]| {
        let mut suffix = b"/".to_vec();
        suffix.extend_from_slice(text);
        suffix
    };
    let new = [suffix(&other[0]), suffix(&other[1])];
    let mut shown = [Vec::new(), Vec::new()];
    for (index, trait_id) in [title_trait, value_trait].into_iter().enumerate() {
        let current = text_of(e, second, trait_id);
        let earlier = added.as_ref().map(|added| added[index].as_slice());
        let mut text = own_text(&current, earlier, &new[index]);
        text.extend_from_slice(&new[index]);
        set_text(e, second, trait_id, &text);
        shown[index] = text;
    }
    set_alpha(e, second, 255.0);
    set_alpha(e, first, 0.0);
    with_state(
        |s| match s.combined.iter_mut().find(|(tile, _)| *tile == second) {
            Some(entry) => entry.1 = new,
            None => s.combined.push((second, new)),
        },
    );
}

/// A combined box's own text: `text` with what was added to it taken off its end, as often as it
/// is there (`earlier`, the last suffix added, and `now`, the one about to be).
pub fn own_text(text: &[u8], earlier: Option<&[u8]>, now: &[u8]) -> Vec<u8> {
    let mut own = text.to_vec();
    loop {
        let before = own.len();
        for suffix in earlier.into_iter().chain([now]) {
            if !suffix.is_empty() && own.ends_with(suffix) && own.len() > suffix.len() {
                own.truncate(own.len() - suffix.len());
            }
        }
        if own.len() == before {
            return own;
        }
    }
}

/// The status box back to its own text, when its pair is over.
fn restore_combined(e: &mut Engine, second: u32) {
    let added = with_state(|s| {
        let index = s.combined.iter().position(|(tile, _)| *tile == second)?;
        Some(s.combined.remove(index).1)
    });
    let Some(added) = added else {
        return;
    };
    let (title_trait, value_trait) = title_value_traits(e);
    for (index, trait_id) in [title_trait, value_trait].into_iter().enumerate() {
        let current = text_of(e, second, trait_id);
        let own = own_text(&current, Some(&added[index]), &[]);
        if own != current {
            set_text(e, second, trait_id, &own);
        }
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
        if cell == 0
            || e.call(TILE_GET_FLOAT, &args![cell, TRAIT_VISIBLE as u32])
                .f32()
                == 0.0
        {
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
    // The shown cells, where they are (the cells and this tile are the display's children, so
    // their x and y are in one frame), and how deep.
    let mut boxes = Vec::new();
    let mut depth = None::<f32>;
    for cell in cells(e, display) {
        if cell == 0
            || e.call(TILE_GET_FLOAT, &args![cell, TRAIT_VISIBLE as u32])
                .f32()
                == 0.0
        {
            continue;
        }
        let x = trait_value(e, cell, TRAIT_X).unwrap_or(0.0);
        let y = trait_value(e, cell, TRAIT_Y).unwrap_or(0.0);
        let width = trait_value(e, cell, TRAIT_WIDTH).unwrap_or(0.0);
        let height = trait_value(e, cell, TRAIT_HEIGHT).unwrap_or(0.0);
        boxes.push(CellBox {
            x,
            y,
            width,
            height,
        });
        if let Some(cell_depth) = trait_value(e, cell, TRAIT_DEPTH) {
            depth = Some(depth.map_or(cell_depth, |d: f32| d.max(cell_depth)));
        }
    }
    // The text's height from its last drawing, else a line of the font a line.
    let text_height = trait_value(e, tile, TRAIT_HEIGHT)
        .filter(|&h| h > 0.0)
        .unwrap_or(LINE_HEIGHT * lines.len() as f32);
    // Where this tile's frame is on the screen: its place there less its own y.
    let own_y = trait_value(e, tile, TRAIT_Y).unwrap_or(0.0);
    let frame_top = fn_00a01440(e, Ptr::new(tile)) - own_y;
    let screen_height = e.call(SCREEN_HEIGHT, &args![]).f64() as f32;
    let placed = place_lines(&boxes, text_height, frame_top, screen_height).unwrap_or_else(|| {
        // No shown cell: under the display, as its height says.
        let height = trait_value(e, display, TRAIT_HEIGHT).unwrap_or(0.0);
        let width = trait_value(e, display, TRAIT_WIDTH).unwrap_or(0.0);
        let locus = trait_value(e, display, TRAIT_LOCUS).unwrap_or(0.0) != 0.0;
        if locus {
            Placement {
                x: 0.0,
                y: height + GAP,
                wrap: width,
            }
        } else {
            let x = trait_value(e, display, TRAIT_X).unwrap_or(0.0);
            let y = trait_value(e, display, TRAIT_Y).unwrap_or(0.0);
            Placement {
                x,
                y: y + height + GAP,
                wrap: width,
            }
        }
    });
    set_value(e, tile, TRAIT_X, placed.x.round());
    set_value(e, tile, TRAIT_Y, placed.y.round());
    if placed.wrap > 0.0 {
        set_value(e, tile, TRAIT_WRAP_WIDTH, placed.wrap.round());
    }
    if let Some(depth) = depth {
        set_value(e, tile, TRAIT_DEPTH, depth);
    }
    set_visible(e, tile, true);
}

/// A shown cell of the display: its place and size in the display's children's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Where the equipped lines go, in the same frame: their x, y and wrap width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub wrap: f32,
}

/// The equipped lines' place: under the lowest shown cell, as wide as the cells; above the
/// highest when under would run past the screen's bottom (`frame_top` is where the frame is on
/// the screen). Placing them by the shown cells, not by the display's height (which in the
/// container and barter menus can reach past the cells, down to or below the frame's and the
/// screen's bottom), keeps them by the stats in every menu. `None` without shown cells.
pub fn place_lines(
    cells: &[CellBox],
    text_height: f32,
    frame_top: f32,
    screen_height: f32,
) -> Option<Placement> {
    let left = cells.iter().map(|c| c.x).reduce(f32::min)?;
    let right = cells.iter().map(|c| c.x + c.width).reduce(f32::max)?;
    let top = cells.iter().map(|c| c.y).reduce(f32::min)?;
    let bottom = cells.iter().map(|c| c.y + c.height).reduce(f32::max)?;
    let below = bottom + GAP;
    let fits_below =
        screen_height <= 0.0 || frame_top + below + text_height <= screen_height - SCREEN_MARGIN;
    let y = if fits_below {
        below
    } else {
        top - GAP - text_height
    };
    Some(Placement {
        x: left,
        y,
        wrap: (right - left).max(0.0),
    })
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

    /// A display as the menu file makes it: `cells` (in file order) and then `extra` children (the
    /// condition meter, the equipped lines), each new child put at the head of the list
    /// (`Tile::SetParent`). Nodes are (next, previous, tile) from `0x1800`; the list at
    /// `display + 4` (head, tail).
    fn display_with(e: &mut Engine, display: u32, tiles: &[u32]) {
        let node_of = |i: usize| 0x1800 + 0x10 * i as u32;
        // In the list, head first: the last child added first.
        let order: Vec<u32> = tiles.iter().rev().copied().collect();
        for (i, tile) in order.iter().enumerate() {
            let node = node_of(i);
            let next = if i + 1 < order.len() {
                node_of(i + 1)
            } else {
                0
            };
            let previous = if i > 0 { node_of(i - 1) } else { 0 };
            e.mem.set_u32(node, next);
            e.mem.set_u32(node + 4, previous);
            e.mem.set_u32(node + 8, *tile);
            e.mem.set_u32(tile + PARENT, display);
        }
        e.mem.set_u32(display + 4, node_of(0));
        e.mem.set_u32(display + 8, node_of(order.len() - 1));
    }

    /// 13 cells at `0x2000 + 0x40 * i`, then the condition meter and an equipped lines' tile.
    fn item_display(e: &mut Engine) -> (u32, Vec<u32>) {
        e.mem.map(0x1000, 0x3000);
        let display = 0x1000;
        let mut tiles: Vec<u32> = (0..13u32).map(|i| 0x2000 + 0x40 * i).collect();
        tiles.push(0x2400);
        tiles.push(0x2440);
        display_with(e, display, &tiles);
        (display, tiles)
    }

    #[test]
    fn the_cells_are_taken_as_the_game_takes_them() {
        let mut e = Engine::new();
        let (display, tiles) = item_display(&mut e);
        let found = cells(&e, display);
        // From the tail along the previous links: the menu file's order, whatever was added after.
        assert_eq!(found.to_vec(), tiles[..13].to_vec());
        assert_eq!(found[CELL_DAM], tiles[11]);
        assert_eq!(found[CELL_DPS], tiles[1]);
        // The card's DAM and DPS (the menus' +0x60, +0x64) and DR, DT are cell pairs; the old reading
        // from the head took them for something else and combined them ("DPS/DAM/DAM...").
        assert!(cell_pair(&e, tiles[11], tiles[1]));
        assert!(cell_pair(&e, tiles[0], tiles[12]));
        assert!(!cell_pair(&e, tiles[2], tiles[3]));
    }

    #[test]
    fn the_equipped_weapon_line_has_its_dam_and_dps() {
        let (mut e, strings, _) = tile_engine();
        let (display, tiles) = item_display(&mut e);
        let (title, value) = (10_000u32, 10_001u32);
        let cells_text = [
            (0, "DR", "--"),
            (1, "DPS", "36"),
            (2, "WG", "3.00"),
            (3, "VAL", "120"),
            (11, "DAM", "18"),
            (12, "DT", "6"),
        ];
        for (cell, t, v) in cells_text {
            strings
                .borrow_mut()
                .insert((tiles[cell], title), t.as_bytes().to_vec());
            strings
                .borrow_mut()
                .insert((tiles[cell], value), v.as_bytes().to_vec());
        }
        // DR hidden (an armour with DT only); every other cell shown.
        e.register(TILE_GET_FLOAT, |e, a| {
            let hidden = [0x2000u32];
            let shown = !(a[1] == TRAIT_VISIBLE as u32 && hidden.contains(&a[0]));
            let _ = e;
            Ret {
                st0: if shown { 1.0 } else { 0.0 },
                ..Ret::default()
            }
        });
        let stats = cell_stats(&mut e, display, &[CELL_DAM, CELL_DPS]);
        assert_eq!(
            equipped_line(b"Weathered 10mm Pistol", &stats),
            b"Equipped: Weathered 10mm Pistol  DAM 18  DPS 36"
        );
        // Armour: the shown one of DR and DT, with its own title.
        let stats = cell_stats(&mut e, display, &[CELL_DR, CELL_DT]);
        assert_eq!(stats, [(b"DT".to_vec(), b"6".to_vec())]);
    }

    #[test]
    fn only_the_inventorys_status_boxes_are_combined() {
        let mut e = Engine::new();
        let (_, tiles) = item_display(&mut e);
        e.mem.map(0x011d_9000, 0x1000);
        let menu = 0x2800;
        e.mem.set_u32(INVENTORY_MENU, menu);
        e.mem.set_u32(menu + STATUS_DR, 0x2900);
        e.mem.set_u32(menu + STATUS_DT, 0x2940);
        assert!(status_pair(&e, 0x2900, 0x2940));
        // The card's DAM and DPS, or DR and DT, never.
        assert!(!status_pair(&e, tiles[11], tiles[1]));
        assert!(!status_pair(&e, tiles[0], tiles[12]));
        // No inventory: nothing is.
        e.mem.set_u32(INVENTORY_MENU, 0);
        assert!(!status_pair(&e, 0x2900, 0x2940));
    }

    #[test]
    fn a_combined_box_never_grows() {
        let (mut e, strings, _) = tile_engine();
        let (title, value) = (10_000u32, 10_001u32);
        // The status line's DR and DT boxes, and a second pair handled in the same frame (as the
        // inventory hands the card's pair and the status line's to the fade one after the other).
        let boxes = [
            (0x1100u32, "DR", "20"),
            (0x1200, "DT", "15"),
            (0x1300, "DAM", "24"),
            (0x1400, "DPS", "192"),
        ];
        for (tile, t, v) in boxes {
            strings
                .borrow_mut()
                .insert((tile, title), t.as_bytes().to_vec());
            strings
                .borrow_mut()
                .insert((tile, value), v.as_bytes().to_vec());
        }
        for _frame in 0..6 {
            show_combined(&mut e, 0x1100, 0x1200);
            show_combined(&mut e, 0x1300, 0x1400);
        }
        assert_eq!(strings.borrow()[&(0x1200, title)], b"DT/DR");
        assert_eq!(strings.borrow()[&(0x1200, value)], b"15/20");
        assert_eq!(strings.borrow()[&(0x1400, title)], b"DPS/DAM");
        // DR changes: its new value replaces the old, nothing piles up.
        strings.borrow_mut().insert((0x1100, value), b"21".to_vec());
        show_combined(&mut e, 0x1100, 0x1200);
        assert_eq!(strings.borrow()[&(0x1200, value)], b"15/21");
        restore_combined(&mut e, 0x1200);
        assert_eq!(strings.borrow()[&(0x1200, value)], b"15");
        assert_eq!(strings.borrow()[&(0x1200, title)], b"DT");
        restore_combined(&mut e, 0x1400);
        assert_eq!(strings.borrow()[&(0x1400, title)], b"DPS");
        with_state(|s| *s = State::default());
    }

    #[test]
    fn own_text_takes_off_what_was_added() {
        assert_eq!(own_text(b"DPS/DAM/DAM/DAM", Some(b"/DAM"), b"/DAM"), b"DPS");
        assert_eq!(own_text(b"15/20", Some(b"/20"), b"/21"), b"15");
        assert_eq!(own_text(b"15", None, b"/20"), b"15");
        // Never to nothing.
        assert_eq!(own_text(b"/20", None, b"/20"), b"/20");
    }

    fn cell(x: f32, y: f32, width: f32, height: f32) -> CellBox {
        CellBox {
            x,
            y,
            width,
            height,
        }
    }

    /// The Pip-Boy's card: room under the stats, the lines go under the lowest shown cell, as wide
    /// as the cells.
    #[test]
    fn in_the_pipboy_the_lines_go_under_the_stats() {
        let cells = [
            cell(0.0, 30.0, 80.0, 24.0),
            cell(90.0, 30.0, 80.0, 24.0),
            cell(0.0, 60.0, 170.0, 24.0),
        ];
        let placed = place_lines(&cells, 24.0, 600.0, 960.0).unwrap();
        assert_eq!(placed.y, 84.0 + GAP);
        assert_eq!((placed.x, placed.wrap), (0.0, 170.0));
    }

    /// A compact container or barter card near the bottom of the screen, whose display reaches
    /// past its cells: the lines come right under the cells, not under the display's height; and
    /// when even that would run past the screen they go above the stats.
    #[test]
    fn in_loot_and_barter_the_lines_stay_on_the_screen() {
        let cells = [cell(10.0, 20.0, 60.0, 20.0), cell(80.0, 44.0, 60.0, 20.0)];
        // The card's frame at 840 of 960: 840 + 68 + 22 fits.
        let placed = place_lines(&cells, 22.0, 840.0, 960.0).unwrap();
        assert_eq!(placed.y, 64.0 + GAP);
        assert!(840.0 + placed.y + 22.0 <= 960.0);
        // At 880 it would not: above the highest shown cell.
        let placed = place_lines(&cells, 22.0, 880.0, 960.0).unwrap();
        assert_eq!(placed.y, 20.0 - GAP - 22.0);
        assert!(placed.y + 22.0 <= 20.0, "not over the stats");
        assert_eq!((placed.x, placed.wrap), (10.0, 130.0));
        // No shown cells: no place from them.
        assert!(place_lines(&[], 22.0, 0.0, 960.0).is_none());
    }
}
