//! Quick loot (`NV_QUICK_LOOT`), a row of the `fixes` preset ([`crate::fixes`], docs/FIXES.md):
//! **not 1:1 with FalloutNV.exe**, an nv-rs addition the player asked for (Patrick, 2026-11-12: "a
//! quick loot system like in Fallout 4", then "model it after this version here ... instead of
//! holding E to open, it should be R"); in the `vanilla` preset, or with `bQuickLoot=0`, Activate
//! opens a container or a corpse as the original does and nothing here runs.
//!
//! **What the player sees.** Looking at a container or a corpse (the HUD's target type 2,
//! `TESObjectREFR::GetTargetType` `00579280`) within the game's activation reach, a panel shows to
//! the right of the crosshair, above the weapon: a dark, see-through box with a thin frame, the
//! container's name on the left of its header (the name the HUD made for it) and the player's
//! weight on the right ("Wg 265/401": actor value `0x2e` and the carry weight, `0093abe0`, as the
//! container menu's weight line reads them), a thin rule, [`ROWS`] lines of items (each the item's
//! own inventory icon, `004be200` as the container menu shows it, its name and "(count)" above
//! one), the highlighted line under a darker bar with a frame and a small square on its left, a
//! scroll bar when there are more, a thin rule, and the keys: "E) Take" on the left, "X) Take All"
//! in the middle (while X is free), "R) Open" on the right, each with the key the player's game
//! really uses. The words are white, and the item names and Take red when taking them is stealing
//! (Take reads Steal then); the frame, rules and highlight are cyan. The box is mostly see-through
//! ([`BACK_ALPHA`]), so the world behind it shows.
//!
//! The panel is the game's own UI: menu XML ([`panel_xml`]) read by the game's reader
//! (`Tile::ParseFile`, `BuildAndNameTree`, `ConnectTraitsToTree`: the steps of `Tile::ReadFile`
//! with the text in memory) into tiles under the HUD's menu tile (`TileRect`, `TileImage`,
//! `TileText`, the game's font 3, `Glow_Monofonto_Medium`, and the interface's `solid.dds`); its
//! texts, icons and places are traits set from here. While it shows, the HUD's own prompt (the
//! action line, the name, the lock and Empty lines, `HUDMainMenu +0x9c`, `+0xa8`, `+0xac`, `+0xb0`)
//! is hidden; its visibility comes back when the panel goes. The world goes on: it is the HUD, not
//! a menu. The panel goes when the player looks away, moves out of reach, or any menu opens; it
//! comes back when they look again.
//!
//! **What the player does.** Read through the game's controls (`Controls`, `00a24660` and the key
//! and mouse queries), so the player's own bindings count:
//! - the wheel (`00a239e0` axis 3; then cleared, so it doesn't also zoom the camera,
//!   `PlayerCharacter`'s `009445b0`) and the Up and Down arrow keys move the highlight;
//! - a tap of Activate (control 5) takes the highlighted stack, whole: one take a press (the take
//!   is on the press going down; holding does nothing more, and nothing opens);
//! - a tap of R ([`OPEN_KEY`]) opens the original menu, through the original Activate (the
//!   container's own lid, sound, menu, and everything that menu does). R is Reload's key: while
//!   the panel shows, R opens and does not reload (its press is taken out of the controls' state,
//!   so nothing after the pick sees it); with no panel, R reloads as before. When R is the
//!   player's Activate key it stays Activate, and Open moves to the Reload control's key
//!   ([`open_key`]);
//! - X takes everything ([`TAKE_ALL_KEY`]).
//!
//! The arrows and X are used only while no control has them (a bound key is the control's); the
//! footer names the keys that work, and leaves out a prompt that has none.
//!
//! **What it covers, and what it leaves to the original.** Only a reference that the original would
//! open straight into its container menu: not locked ([`eligible`]: a locked one goes to the key
//! or the lock pick menu as before), no script on the reference (its `OnActivate` may do anything,
//! a quest stage, a message: those keep the original's Activate), not an ash pile (they activate
//! another reference), and not empty (the original's "Empty" line shows). Items on the ground,
//! living actors (pickpocketing) and companions are the original's.
//!
//! **Taking, by the container menu's rules** (`ContainerMenu::TakeAllFn` `0075da60` and
//! `ItemSelectCallback` `0075dc80`, which need the menu open, so their steps are made here): the
//! container's items as the menu lists them (`00575510`, `004ca330`, its first-pass filter: playable
//! objects, playable biped models, playable weapons and ammunition); an item dropped by someone who
//! is neither the player nor a follower goes back through the player's slot `+0x3d0`, as the menu
//! does; every other stack moves through the container's `RemoveItem` (slot `+0x17c`: object, its
//! first extra list, count, stolen, 0, the player, 0, 0, 1, 0) with the actor's pick-up sound
//! (`008aded0`), or the menu's take-all sound. Stealing is decided as the menu's opener decides it
//! (`00704ed0`: a reference that is not an actor, has an owner, and the player is not one of its
//! owners): the items are taken as stolen (their owner kept), and the stolen value
//! (`ContainerMenu::GetStolenValue`'s rule, unowned items counting) goes to `Actor::StealAlarm`, as
//! the menu sends it when it closes, once per take. Quest items, weight and the rest are the
//! transfer's own.
//!
//! **Where it runs.** After the crosshair pick (`0070bc20`; it returns at once in menu mode), which
//! main runs after the controls are polled and after the player's update (at the end of
//! `AITaskManager::Process` with the split update, `011c3ea4`, else from
//! `Main::UpdateNonRenderSafeAITasks`), so a press is seen here before the player's next update
//! acts on it, and the wheel and R cleared here are not read by it; and in front of
//! `TESObjectREFR::Activate` (`00573170`): the one Activate the player's next update makes from an
//! Activate press that took an item here is not passed on. Every other Activate goes through as it
//! is.

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::bsmenu::tile::{
    fn_00a011b0, tile_build_and_name_tree, tile_connect_traits_to_tree, tile_get_child_by_name,
    tile_get_value, tile_parse_file, BuildStorage,
};
use engine::units::unplaced::run_0070bbe0::fn_0070bc20;
use std::sync::Mutex;

/// The crosshair pick (`InterfaceManager`, `0070bc20`) and `TESObjectREFR::Activate` (`00573170`).
pub const PICK: u32 = 0x0070_bc20;
pub const ACTIVATE: u32 = 0x0057_3170;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "quick loot",
    env: "NV_QUICK_LOOT",
    addresses: &[PICK, ACTIVATE],
    summary: "looking at a container or a corpse shows its items in a panel on the HUD; Activate takes one, X takes all, R opens the original menu",
    install,
};

/// Lines of items shown at once.
pub const ROWS: usize = 5;
/// The Open key (`DIK_R`), the Take All key (`DIK_X`) and the arrows (`DIK_UP`, `DIK_DOWN`).
pub const OPEN_KEY: u8 = 0x13;
pub const TAKE_ALL_KEY: u8 = 0x2d;
const KEY_UP: u8 = 0xc8;
const KEY_DOWN: u8 = 0xd0;
/// Frames between two readings of a container's items while it is looked at (another hand may
/// change them).
const REREAD_FRAMES: u32 = 30;

// The panel's measures, in the interface's units (the screen is 960 high).

/// The panel's width (its height is [`PANEL_HEIGHT`], under the footer).
pub const PANEL_WIDTH: u32 = 480;
/// The panel's left edge right of the crosshair, and its top above the crosshair.
const PANEL_RIGHT_OF_CROSSHAIR: f32 = 60.0;
const PANEL_ABOVE_CROSSHAIR: f32 = 160.0;
/// The margin, the header rule's y, the first row's top and a row's height.
const MARGIN: u32 = 12;
const HEADER_RULE_Y: u32 = 34;
const ROWS_TOP: u32 = 40;
const ROW_HEIGHT: u32 = 29;
/// The icon's box in a row (2:1, the shape of the game's inventory images) and the name's x.
const ICON_X: u32 = 30;
const ICON_Y: u32 = 3;
const ICON_WIDTH: f32 = 46.0;
const ICON_HEIGHT: f32 = 23.0;
const NAME_X: u32 = 84;
/// The scroll bar's x, the footer rule's y and the footer's text y.
const SCROLL_X: u32 = PANEL_WIDTH - 10;
const FOOTER_RULE_Y: u32 = ROWS_TOP + ROWS as u32 * ROW_HEIGHT + 5;
const FOOTER_Y: u32 = FOOTER_RULE_Y + 6;
/// The panel's height: the footer's line and a margin.
pub const PANEL_HEIGHT: u32 = FOOTER_Y + ROW_HEIGHT - 2;
/// The longest row and header names, in characters (the font is monospaced): longer ones end
/// in "...".
pub const ROW_CHARACTERS: usize = 27;
pub const HEADER_CHARACTERS: usize = 22;
/// The colors: white words, red ones for what taking would steal, the Pip-Boy's cyan for the
/// frame, rules and highlight.
const WHITE: [u8; 3] = [255, 255, 255];
const RED: [u8; 3] = [255, 72, 64];
const CYAN: [u8; 3] = [110, 215, 240];
/// How opaque the parts are (0 to 255): the box mostly see-through, the highlight's bar a little
/// darker, the lines clear.
pub const BACK_ALPHA: u32 = 72;
const BAR_ALPHA: u32 = 60;
const FRAME_ALPHA: u32 = 170;
const RULE_ALPHA: u32 = 130;
const HIGHLIGHT_ALPHA: u32 = 210;
/// The game's font for all of it: font 3, `Glow_Monofonto_Medium`.
const FONT: u32 = 3;

// The game's words and functions.
const PLAYER: u32 = 0x011d_ea3c;
const HUD: u32 = 0x011d_96c0;
/// `Menu::pMenu`, the menu's tile.
const MENU_TILE: u32 = 0x04;
/// `HUDMainMenu`'s prompt tiles: the action line (with its button images), the name, the lock line
/// and the Empty line.
const HUD_PROMPT: [u32; 4] = [0x9c, 0xa8, 0xac, 0xb0];
/// The name `HUDMainMenu::SetInfoForRef` made for the reference it shows (`char[0x104]`), and a
/// reference's own full name (`0055d520`) when that is empty.
const HUD_TARGET_NAME: u32 = 0x011d_9c48;
const REFERENCE_NAME: u32 = 0x0055_d520;
/// Traits.
const TRAIT_X: i32 = 0xfa1;
const TRAIT_Y: i32 = 0xfa2;
const TRAIT_VISIBLE: i32 = 0xfa3;
const TRAIT_ALPHA: i32 = 0xfa9;
const TRAIT_HEIGHT: i32 = 0xfb0;
const TRAIT_WIDTH: i32 = 0xfb1;
const TRAIT_RED: i32 = 0xfb2;
const TRAIT_GREEN: i32 = 0xfb3;
const TRAIT_BLUE: i32 = 0xfb4;
const TRAIT_ZOOM: i32 = 0xfb8;
const TRAIT_STRING: i32 = 0xfc4;
const TRAIT_FILENAME: i32 = 0xfcc;
const TRAIT_FILE_WIDTH: i32 = 0xfcd;
const TRAIT_FILE_HEIGHT: i32 = 0xfce;
const TILE_GET_STRING: u32 = 0x00a0_11f0;
const TILE_SET_FLOAT: u32 = 0x00a0_12d0;
const TILE_SET_STRING: u32 = 0x00a0_1350;
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
/// The interface's default texture atlas name (a `char *`), the one that holds `solid.dds`.
const DEFAULT_ATLAS_NAME: u32 = 0x011f_336c;
/// The interface's screen size (`InterfaceManager::GetScreenWidth`, `GetScreenHeight`).
const SCREEN_WIDTH: u32 = 0x0071_5d40;
const SCREEN_HEIGHT: u32 = 0x0071_5da0;
/// `HUDMainMenu::RefreshInfoForRef` (`00778b10`): the prompt made again for the last reference.
const HUD_REFRESH: u32 = 0x0077_8b10;
/// In menu mode (`007023a0`, the manager), the activate reference (`00703350`), and the count the
/// player's Activate passes (`00778930`).
const MENU_MODE: u32 = 0x0070_23a0;
const ACTIVATE_REF: u32 = 0x0070_3350;
const ACTIVATE_COUNT: u32 = 0x0077_8930;
/// `TESObjectREFR::GetTargetType` and its container-or-corpse answer.
const TARGET_TYPE: u32 = 0x0057_9280;
const CONTAINER_OR_CORPSE: u32 = 2;
/// The lock (`00569160`) and whether it is locked (`005021a0`).
const GET_LOCK: u32 = 0x0056_9160;
const IS_LOCKED: u32 = 0x0050_21a0;
/// A reference's extra list (`005d43c0`), `BaseExtraList::GetExtraData` (`00410220`) and the script
/// extra's type.
const EXTRA_LIST: u32 = 0x005d_43c0;
const GET_EXTRA_DATA: u32 = 0x0041_0220;
const EXTRA_SCRIPT: u32 = 0x0d;
/// The two ash pile base forms (their words), a reference's base form (`+0x20`).
const ASH_PILES: [u32; 2] = [0x011c_a27c, 0x011c_a280];
const BASE_FORM: u32 = 0x20;
/// Items: the count of a reference's (`00575590`), the walk (`00575510` with its iterator and
/// changes, `004ca330` the next item, `004bf630` the iterator's delete), an item's name
/// (`004be2d0`), icon (`ItemChange::GetIconOne`, `004be200`) and delete (`004459e0`).
const ITEM_TOTAL: u32 = 0x0057_5590;
const INVENTORY_START: u32 = 0x0057_5510;
const NEXT_ITEM: u32 = 0x004c_a330;
const ITERATOR_DELETE: u32 = 0x004b_f630;
const ITEM_FULL_NAME: u32 = 0x004b_e2d0;
const ITEM_ICON: u32 = 0x004b_e200;
const ITEM_DELETE: u32 = 0x0044_59e0;
/// The first-pass filter's calls (`ContainerMenu::FirstPassFilterFunc`, `0075cfc0`).
const FORM_TYPE: u32 = 0x0040_1170;
const BIPED_MODEL_OF: u32 = 0x0048_0db0;
const BIPED_PLAYABLE: u32 = 0x0048_0d90;
const RT_DYNAMIC_CAST: u32 = 0x00ec_43fb;
const TYPE_FORM_BASE: u32 = 0x0118_3108;
const TYPE_PLAYABLE_FORM: u32 = 0x0118_3158;
const FORM_TEST_0048CEE0: u32 = 0x0048_cee0;
const WEAPON_TEST_0047BCF0: u32 = 0x0047_bcf0;
const AMMO_TEST_004C94D0: u32 = 0x004c_94d0;
const WEAPON: u32 = 0x28;
const AMMO: u32 = 0x29;
/// The transfer: an extra list's dropper (`0041de00`), the dropper's two form tests (`00440da0`,
/// `00440d80`), the player's give-back slot, `RemoveItem`'s slot.
const ITEM_DROPPER: u32 = 0x0041_de00;
const FORM_FLAG_800: u32 = 0x0044_0da0;
const FORM_FLAG_20: u32 = 0x0044_0d80;
const GIVE_BACK_SLOT: u32 = 0x3d0;
const REMOVE_ITEM_SLOT: u32 = 0x17c;
const IS_ACTOR_SLOT: u32 = 0x100;
/// Stealing: the owner (`00567790`), `IsAnOwner` (`005785e0`), an item's value (`004bd400`), an
/// extra list's owner (`00418660`) and count (`00418770`), `Actor::StealAlarm` (`008bfa40`).
const OWNER: u32 = 0x0056_7790;
const IS_AN_OWNER: u32 = 0x0057_85e0;
const ITEM_VALUE: u32 = 0x004b_d400;
const EXTRA_OWNER: u32 = 0x0041_8660;
const EXTRA_COUNT: u32 = 0x0041_8770;
const STEAL_ALARM: u32 = 0x008b_fa40;
/// The player's weight and carry weight (`0093abe0`: actor value `0x2e` into the first float,
/// `008a0c20` into the second) and the weight label's setting (the container menu's weight line's).
const PLAYER_WEIGHTS: u32 = 0x0093_abe0;
const WEIGHT_LABEL: u32 = 0x011d_1e60;
/// Sounds: the actor's pick-up sound (`008aded0`), and a named sound (`BSAudio::QInstance`,
/// `GetSoundHandleByName`, `Play`, the handle's destructor) for the menu's take-all sound.
const PICKUP_SOUND: u32 = 0x008a_ded0;
const AUDIO_INSTANCE: u32 = 0x0045_3a70;
const AUDIO_SOUND_BY_NAME: u32 = 0x00ad_7550;
const SOUND_HANDLE_PLAY: u32 = 0x00ad_8830;
const SOUND_HANDLE_DESTRUCTOR: u32 = 0x0048_3710;
const TAKE_ALL_SOUND: u32 = 0x0107_22c0;
/// The controls: their object (`00877720` of the word at `011dea0c`), a control's state
/// (`00a24660`: mode 1 went down), a key's (`00a24180`), a mouse axis (`00a239e0`, 3 the wheel) and
/// the wheel's word (`+0x1b2c`), the keyboard's state and last state (`+0x18f8`, `+0x19f8`, a byte
/// a key, bit 7 down), a control's bindings (`00a24f10`: the keyboard key in bits 16..24, 0xff
/// unbound), the key names (`011d52f0`, settings) and a setting's text (`00403df0`).
const CONTROLS_OWNER: u32 = 0x011d_ea0c;
const CONTROLS_GET: u32 = 0x0087_7720;
const CONTROL_STATE: u32 = 0x00a2_4660;
const KEY_STATE: u32 = 0x00a2_4180;
const MOUSE_AXIS: u32 = 0x00a2_39e0;
const WHEEL_WORD: u32 = 0x1b2c;
const KEYBOARD_STATE: u32 = 0x18f8;
const KEYBOARD_LAST: u32 = 0x19f8;
const BINDINGS: u32 = 0x00a2_4f10;
const UNBOUND: u8 = 0xff;
const KEY_NAMES: u32 = 0x011d_52f0;
const SETTING_TEXT: u32 = 0x0040_3df0;
const ACTIVATE_CONTROL: u32 = 5;
const RELOAD_CONTROL: u32 = 7;
const CONTROL_COUNT: u32 = 0x1c;

/// One line of the list: the object, which of its stacks (the walk gives one item a stack), its
/// count, its name and its icon's file.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub form: u32,
    pub stack: u32,
    pub count: i32,
    pub name: Vec<u8>,
    pub icon: Vec<u8>,
}

/// The panel's tiles (0 for one the reader did not make).
#[derive(Clone, Debug, Default)]
struct Panel {
    /// The HUD and its menu tile the panel was made under, and the panel's own tile.
    hud: u32,
    menu_tile: u32,
    root: u32,
    title: u32,
    weight: u32,
    highlight: u32,
    rows: [u32; ROWS],
    icons: [u32; ROWS],
    names: [u32; ROWS],
    track: u32,
    thumb: u32,
    take: u32,
    take_all: u32,
    open: u32,
}

/// An icon tile's file and whether it has been fitted to its box (after the game read its size).
#[derive(Clone, Debug, Default)]
struct Icon {
    file: Vec<u8>,
    fitted: bool,
}

/// What the panel keeps between frames.
#[derive(Debug, Default)]
struct State {
    /// The reference listed, its rows, whether taking from it is stealing, the highlight and the
    /// first line shown.
    target: u32,
    rows: Vec<Row>,
    stealing: bool,
    highlight: usize,
    top: usize,
    /// Frames since the rows were read, and whether they must be read again.
    age: u32,
    dirty: bool,
    /// Whether the panel shows, and the HUD prompt tiles hidden for it with the visibility each
    /// had (`None`: no visibility trait, which shows).
    shown: bool,
    hidden: Vec<(u32, Option<f32>)>,
    /// The panel's icon tiles' files (kept while the panel lives).
    icons: [Icon; ROWS],
    /// The reference whose next Activate by the player is not passed on (its press took an item),
    /// and whether that was set at this pick (it lasts until the next one).
    swallow: Option<(u32, bool)>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
/// The panel lives as long as the HUD's tiles; it is kept apart from [`State`], which is reset
/// whenever the panel goes.
static PANEL: Mutex<Option<Panel>> = Mutex::new(None);

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(State::default))
}

fn current_panel() -> Option<Panel> {
    PANEL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn keep_panel(panel: Option<Panel>) {
    *PANEL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = panel;
}

/// Registers the pick and `Activate`.
pub fn install(e: &mut Engine) {
    e.register(PICK, |e, a| {
        fn_0070bc20(e, Ptr::new(a[0]));
        after_pick(e, a[0]);
        Ret::default()
    });
    e.register(ACTIVATE, |e, a| {
        let player = e.global::<u32>(PLAYER);
        let swallowed = with_state(|s| match s.swallow {
            Some((target, _)) if a[1] != 0 && a[1] == player && target == a[0] => {
                s.swallow = None;
                true
            }
            _ => false,
        });
        let activated = swallowed
            || engine::units::fallout_shared::tesobjectrefr_p4::tes_object_refr_activate(
                e,
                Ptr::new(a[0]),
                a[1],
                a[2] as u8,
                a[3],
                a[4],
            );
        Ret {
            eax: u32::from(activated),
            ..Ret::default()
        }
    });
}

// The parts that need no game: the panel's XML, its texts, the highlight, the keys.

/// The panel as menu XML for the game's reader: a frame (a `TileRect` with see-through
/// `solid.dds` images from the interface's atlas `atlas`), the header's two texts and rule, the
/// highlight (a darker bar, its frame and the small square), [`ROWS`] rows of an icon and a name, a
/// scroll bar, the footer's rule and three prompts. The tiles are named `nvrs_ql_*`; the panel
/// starts hidden, and the texts, icons and places are set from code.
pub fn panel_xml(atlas: &str) -> String {
    let width = PANEL_WIDTH;
    let height = PANEL_HEIGHT;
    let solid = |name: &str, x: u32, y: u32, w: u32, h: u32, color: [u8; 3], alpha: u32| {
        format!(
            "<image name=\"{name}\"><filename>solid.dds</filename><texatlas>{atlas}</texatlas>\
             <x>{x}</x><y>{y}</y><width>{w}</width><height>{h}</height>\
             <red>{}</red><green>{}</green><blue>{}</blue><alpha>{alpha}</alpha></image>\n",
            color[0], color[1], color[2]
        )
    };
    let text = |name: &str, x: u32, y: u32| {
        format!(
            "<text name=\"{name}\"><x>{x}</x><y>{y}</y><font>{FONT}</font>\
             <red>{}</red><green>{}</green><blue>{}</blue><alpha>255</alpha></text>\n",
            WHITE[0], WHITE[1], WHITE[2]
        )
    };
    let black = [0, 0, 0];
    let rule = width - 2 * MARGIN;
    let bar = SCROLL_X - 6 - 8;
    let mut xml = format!(
        "<rect name=\"nvrs_ql_panel\"><locus>1</locus><x>0</x><y>0</y><width>{width}</width>\
         <height>{height}</height><depth>8</depth><visible>0</visible>\n"
    );
    // The box: dark and see-through, a thin frame.
    xml += &solid("nvrs_ql_back", 0, 0, width, height, black, BACK_ALPHA);
    xml += &solid("nvrs_ql_frame_top", 0, 0, width, 2, CYAN, FRAME_ALPHA);
    xml += &solid(
        "nvrs_ql_frame_bottom",
        0,
        height - 2,
        width,
        2,
        CYAN,
        FRAME_ALPHA,
    );
    xml += &solid("nvrs_ql_frame_left", 0, 0, 2, height, CYAN, FRAME_ALPHA);
    xml += &solid(
        "nvrs_ql_frame_right",
        width - 2,
        0,
        2,
        height,
        CYAN,
        FRAME_ALPHA,
    );
    // The header: the name, the weight (placed from the right by code), the rule.
    xml += &text("nvrs_ql_title", MARGIN, 5);
    xml += &text("nvrs_ql_weight", width - MARGIN - 140, 5);
    xml += &solid(
        "nvrs_ql_rule",
        MARGIN,
        HEADER_RULE_Y,
        rule,
        2,
        CYAN,
        RULE_ALPHA,
    );
    // The highlight, moved to its row by code.
    xml += &format!(
        "<rect name=\"nvrs_ql_highlight\"><locus>1</locus><x>0</x><y>{ROWS_TOP}</y>\
         <width>{width}</width><height>{ROW_HEIGHT}</height><visible>0</visible>\n"
    );
    xml += &solid("nvrs_ql_bar", 8, 1, bar, ROW_HEIGHT - 2, black, BAR_ALPHA);
    xml += &solid("nvrs_ql_bar_top", 8, 0, bar, 2, CYAN, HIGHLIGHT_ALPHA);
    xml += &solid(
        "nvrs_ql_bar_bottom",
        8,
        ROW_HEIGHT - 2,
        bar,
        2,
        CYAN,
        HIGHLIGHT_ALPHA,
    );
    xml += &solid(
        "nvrs_ql_bar_left",
        8,
        0,
        2,
        ROW_HEIGHT,
        CYAN,
        HIGHLIGHT_ALPHA,
    );
    xml += &solid(
        "nvrs_ql_bar_right",
        8 + bar - 2,
        0,
        2,
        ROW_HEIGHT,
        CYAN,
        210,
    );
    xml += &solid("nvrs_ql_marker", 15, ROW_HEIGHT / 2 - 4, 8, 8, CYAN, 255);
    xml += "</rect>\n";
    // The rows: an icon (fitted to its box by code) and a name.
    for row in 0..ROWS {
        let y = ROWS_TOP + row as u32 * ROW_HEIGHT;
        xml += &format!(
            "<rect name=\"nvrs_ql_row{row}\"><locus>1</locus><x>0</x><y>{y}</y>\
             <width>{width}</width><height>{ROW_HEIGHT}</height><visible>0</visible>\n\
             <image name=\"nvrs_ql_icon{row}\"><x>{ICON_X}</x><y>{ICON_Y}</y><width>{}</width>\
             <height>{}</height><zoom>100</zoom><alpha>0</alpha></image>\n",
            ICON_WIDTH as u32, ICON_HEIGHT as u32
        );
        xml += &text(&format!("nvrs_ql_name{row}"), NAME_X, 2);
        xml += "</rect>\n";
    }
    // The scroll bar: its track and its thumb (sized and placed by code).
    let track = ROWS as u32 * ROW_HEIGHT;
    xml += &solid("nvrs_ql_track", SCROLL_X, ROWS_TOP, 4, track, CYAN, 60);
    xml += &solid("nvrs_ql_thumb", SCROLL_X, ROWS_TOP, 4, track, CYAN, 220);
    // The footer: the rule and the prompts (Take All centered, Open from the right, by code).
    xml += &solid(
        "nvrs_ql_footer_rule",
        MARGIN,
        FOOTER_RULE_Y,
        rule,
        2,
        CYAN,
        140,
    );
    xml += &text("nvrs_ql_take", MARGIN, FOOTER_Y);
    xml += &text("nvrs_ql_take_all", width / 2 - 60, FOOTER_Y);
    xml += &text("nvrs_ql_open", width - MARGIN - 90, FOOTER_Y);
    xml += "</rect>\n";
    xml
}

/// `text` cut to `limit` characters, the cut ones replaced by "...".
pub fn fit(text: &[u8], limit: usize) -> Vec<u8> {
    if text.len() <= limit {
        return text.to_vec();
    }
    let mut cut = text[..limit.saturating_sub(3)].to_vec();
    while cut.last() == Some(&b' ') {
        cut.pop();
    }
    cut.extend_from_slice(b"...");
    cut
}

/// A row's text: the name, and "(count)" above one (the count is never cut).
pub fn row_label(row: &Row) -> Vec<u8> {
    let count = if row.count > 1 {
        format!(" ({})", row.count).into_bytes()
    } else {
        Vec::new()
    };
    let mut label = fit(&row.name, ROW_CHARACTERS.saturating_sub(count.len()));
    label.extend_from_slice(&count);
    label
}

/// The header's weight: the label, the player's weight and carry weight, rounded ("Wg 265/401").
pub fn weight_line(label: &[u8], carried: f32, capacity: f32) -> Vec<u8> {
    let mut line = label.to_vec();
    line.extend_from_slice(
        format!(" {}/{}", carried.round() as i32, capacity.round() as i32).as_bytes(),
    );
    line
}

/// The keys' names for the footer: Activate's, Take All's when it is free, Open's when there is
/// one.
pub struct Keys {
    pub activate: Vec<u8>,
    pub take_all: Option<Vec<u8>>,
    pub open: Option<Vec<u8>>,
}

/// The footer: Take (Steal when taking is stealing), Take All and Open, each "key) action"; a
/// prompt without a key is empty.
pub fn prompts(keys: &Keys, stealing: bool) -> [Vec<u8>; 3] {
    let prompt = |key: &[u8], action: &[u8]| {
        let mut text = key.to_vec();
        text.extend_from_slice(b") ");
        text.extend_from_slice(action);
        text
    };
    let take = prompt(
        &keys.activate,
        if stealing { b"Steal" as &[u8] } else { b"Take" },
    );
    let take_all = keys
        .take_all
        .as_ref()
        .map_or_else(Vec::new, |key| prompt(key, b"Take All"));
    let open = keys
        .open
        .as_ref()
        .map_or_else(Vec::new, |key| prompt(key, b"Open"));
    [take, take_all, open]
}

/// What the panel shows: the header, the [`ROWS`] lines (`None` for an empty line), which line is
/// highlighted, the scroll bar's thumb (its top and length within the track, or `None` when every
/// row shows) and the footer.
#[derive(Debug, PartialEq)]
pub struct View {
    pub title: Vec<u8>,
    pub weight: Vec<u8>,
    pub lines: Vec<Option<Vec<u8>>>,
    pub highlight: Option<usize>,
    pub thumb: Option<(f32, f32)>,
    pub footer: [Vec<u8>; 3],
}

/// The panel's contents for `rows` with the highlight and the first line shown.
pub fn compose(
    title: &[u8],
    weight: &[u8],
    rows: &[Row],
    highlight: usize,
    top: usize,
    keys: &Keys,
    stealing: bool,
) -> View {
    let lines = (0..ROWS)
        .map(|line| rows.get(top + line).map(row_label))
        .collect();
    let track = (ROWS as u32 * ROW_HEIGHT) as f32;
    let thumb = (rows.len() > ROWS).then(|| {
        let length = (track * ROWS as f32 / rows.len() as f32).max(12.0);
        let room = rows.len() - ROWS;
        let offset = (track - length) * top.min(room) as f32 / room as f32;
        (offset, length)
    });
    View {
        title: fit(title, HEADER_CHARACTERS),
        weight: weight.to_vec(),
        lines,
        highlight: highlight.checked_sub(top).filter(|&line| line < ROWS),
        thumb,
        footer: prompts(keys, stealing),
    }
}

/// The highlight moved by `step` lines inside `count` rows, and the first line shown so that it
/// is among the [`ROWS`] shown (and no line is left empty that a row could fill).
pub fn scroll(highlight: usize, top: usize, step: i32, count: usize) -> (usize, usize) {
    if count == 0 {
        return (0, 0);
    }
    let highlight = (highlight as i64 + i64::from(step)).clamp(0, count as i64 - 1) as usize;
    let mut top = top.min(count.saturating_sub(ROWS));
    if highlight < top {
        top = highlight;
    } else if highlight >= top + ROWS {
        top = highlight + 1 - ROWS;
    }
    (highlight, top)
}

/// The Open key: R, unless R is Activate's key (Activate keeps it); then the Reload control's key,
/// unless that is Activate's too or unbound; else none. `activate` and `reload` are the controls'
/// keyboard keys.
pub fn open_key(activate: Option<u8>, reload: Option<u8>) -> Option<u8> {
    if activate != Some(OPEN_KEY) {
        Some(OPEN_KEY)
    } else {
        reload.filter(|&key| Some(key) != activate)
    }
}

/// This frame's presses while the panel shows (a key that went down this frame; a held key is not
/// a press).
#[derive(Clone, Copy, Debug, Default)]
pub struct Presses {
    pub activate: bool,
    pub open: bool,
    pub take_all: bool,
}

/// What the panel does with a frame's presses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Take,
    TakeAll,
    Open,
}

/// Open first (the menu shows everything), then Take All, then Take. Holding Activate makes no
/// press after the first frame, so it neither opens nor takes again.
pub fn action(presses: Presses) -> Action {
    if presses.open {
        Action::Open
    } else if presses.take_all {
        Action::TakeAll
    } else if presses.activate {
        Action::Take
    } else {
        Action::None
    }
}

// The game.

/// After each pick: the list kept, shown, hidden, and the keys read (module documentation).
fn after_pick(e: &mut Engine, manager: u32) {
    // The Activate taken at the last pick has had its update.
    with_state(|s| {
        s.swallow = match s.swallow {
            Some((target, true)) => Some((target, false)),
            _ => None,
        }
    });
    if manager == 0 {
        return;
    }
    if e.call(MENU_MODE, &args![manager]).bool() {
        hide(e);
        return;
    }
    let target = e.call(ACTIVATE_REF, &args![]).u32();
    if !eligible(e, target) {
        hide(e);
        return;
    }
    let (changed, reread) = with_state(|s| {
        let changed = s.target != target;
        s.age += 1;
        let reread = changed || s.dirty || s.age >= REREAD_FRAMES;
        (changed, reread)
    });
    if changed {
        hide(e);
        with_state(|s| s.target = target);
    }
    if reread {
        let rows = rows(e, target);
        let stealing = stealing(e, target).is_some();
        with_state(|s| {
            s.rows = rows;
            s.stealing = stealing;
            s.age = 0;
            s.dirty = false;
            let (highlight, top) = scroll(s.highlight, s.top, 0, s.rows.len());
            s.highlight = highlight;
            s.top = top;
        });
    }
    if with_state(|s| s.rows.is_empty()) {
        hide(e);
        return;
    }
    read_keys(e, target);
    if with_state(|s| s.rows.is_empty() || s.target != target) {
        hide(e);
        return;
    }
    show(e);
}

/// Whether `target` is a container or corpse quick loot takes from (module documentation).
pub fn eligible(e: &mut Engine, target: u32) -> bool {
    if target == 0 || target == e.global::<u32>(PLAYER) {
        return false;
    }
    if e.call(TARGET_TYPE, &args![target]).u32() != CONTAINER_OR_CORPSE {
        return false;
    }
    let base = e.mem.u32(target + BASE_FORM);
    if base == 0 || ASH_PILES.iter().any(|&pile| e.global::<u32>(pile) == base) {
        return false;
    }
    let lock = e.call(GET_LOCK, &args![target]).u32();
    if lock != 0 && e.call(IS_LOCKED, &args![lock]).bool() {
        return false;
    }
    let extra = e.call(EXTRA_LIST, &args![target]).u32();
    if extra == 0 || e.call(GET_EXTRA_DATA, &args![extra, EXTRA_SCRIPT]).u32() != 0 {
        return false;
    }
    e.call(ITEM_TOTAL, &args![target, 0u32, 0u32]).i32() > 0
}

/// Every item of `target` the container menu would list, in its walk's order: the walk's own
/// copies (`004ca330` makes each), kept until [`free`]; the others are freed at once.
fn collect(e: &mut Engine, target: u32) -> Vec<u32> {
    let mut items = Vec::new();
    e.with_stack(8, |e, slots| {
        let iterator_slot = slots.addr();
        let changes_slot = slots.addr() + 4;
        e.mem.set_u32(iterator_slot, 0);
        e.mem.set_u32(changes_slot, 0);
        e.call(
            INVENTORY_START,
            &args![target, 0u32, iterator_slot, changes_slot],
        );
        let iterator = e.mem.u32(iterator_slot);
        let changes = e.mem.u32(changes_slot);
        if iterator != 0 && changes != 0 {
            loop {
                let item = e.call(NEXT_ITEM, &args![changes, iterator]).u32();
                if item == 0 {
                    break;
                }
                if listed(e, item) {
                    items.push(item);
                } else {
                    e.call(ITEM_DELETE, &args![item, 1u32]);
                }
            }
        }
        if iterator != 0 {
            e.call(ITERATOR_DELETE, &args![iterator, 1u32]);
        }
    });
    items
}

/// The copies [`collect`] kept, deleted as the container menu deletes its list's.
fn free(e: &mut Engine, items: &[u32]) {
    for &item in items {
        e.call(ITEM_DELETE, &args![item, 1u32]);
    }
}

/// The container menu's first pass, the loot mode's (`0075cfc0`): an item with an object that is a
/// playable form with a playable biped model, and a playable weapon or ammunition.
fn listed(e: &mut Engine, item: u32) -> bool {
    let object = e.mem.u32(item + 8);
    if object == 0 || e.mem.u32(item + 4) as i32 <= 0 {
        return false;
    }
    let biped = e.call(BIPED_MODEL_OF, &args![object]).u32();
    if biped != 0 && !e.call(BIPED_PLAYABLE, &args![biped]).bool() {
        return false;
    }
    let playable = e
        .call(
            RT_DYNAMIC_CAST,
            &args![object, 0u32, TYPE_FORM_BASE, TYPE_PLAYABLE_FORM, 0u32],
        )
        .u32();
    if playable == 0 || e.call(FORM_TEST_0048CEE0, &args![playable]).u32() == 0 {
        return false;
    }
    let kind = e.call(FORM_TYPE, &args![object]).u32() & 0xff;
    if kind == WEAPON && !e.call(WEAPON_TEST_0047BCF0, &args![object]).bool() {
        return false;
    }
    !(kind == AMMO && !e.call(AMMO_TEST_004C94D0, &args![object]).bool())
}

/// A string the game returned, or nothing for null.
fn game_string(e: &Engine, text: u32) -> Vec<u8> {
    if text == 0 {
        Vec::new()
    } else {
        e.mem.cstr(text)
    }
}

/// `target`'s rows (the stacks [`collect`] gives, numbered within each object).
fn rows(e: &mut Engine, target: u32) -> Vec<Row> {
    let items = collect(e, target);
    let mut rows: Vec<Row> = Vec::new();
    for &item in &items {
        let form = e.mem.u32(item + 8);
        let stack = rows.iter().filter(|r| r.form == form).count() as u32;
        let name = e.call(ITEM_FULL_NAME, &args![item]).u32();
        let icon = e.call(ITEM_ICON, &args![item, 0u32]).u32();
        rows.push(Row {
            form,
            stack,
            count: e.mem.u32(item + 4) as i32,
            name: game_string(e, name),
            icon: game_string(e, icon),
        });
    }
    free(e, &items);
    rows
}

/// Which of `items` to take: `row`'s stack (the same object, the same place among that object's
/// stacks), or all of them with `None`.
pub fn chosen(forms: &[u32], row: Option<(u32, u32)>) -> Vec<usize> {
    let mut chosen = Vec::new();
    for (index, &form) in forms.iter().enumerate() {
        let stack = forms[..index].iter().filter(|&&f| f == form).count() as u32;
        match row {
            Some((want, want_stack)) if want != form || want_stack != stack => {}
            Some(_) => {
                chosen.push(index);
                break;
            }
            None => chosen.push(index),
        }
    }
    chosen
}

/// The controls object.
fn controls(e: &mut Engine) -> u32 {
    let owner = e.global::<u32>(CONTROLS_OWNER);
    e.call(CONTROLS_GET, &args![owner]).u32()
}

/// The keyboard key of `control`, if it has one.
fn control_key(e: &mut Engine, controls: u32, control: u32) -> Option<u8> {
    let key = (e.call(BINDINGS, &args![controls, control]).u32() >> 16) as u8;
    (key != UNBOUND).then_some(key)
}

/// Whether keyboard key `key` is bound to any of the game's controls.
fn bound(e: &mut Engine, controls: u32, key: u8) -> bool {
    (0..CONTROL_COUNT).any(|control| control_key(e, controls, control) == Some(key))
}

/// Whether a key went down this frame.
fn key_down(e: &mut Engine, controls: u32, key: u8) -> bool {
    e.call(KEY_STATE, &args![controls, u32::from(key), 1u32])
        .bool()
}

/// Whether a free key went down (a bound key is the control's).
fn free_key_down(e: &mut Engine, controls: u32, key: u8) -> bool {
    !bound(e, controls, key) && key_down(e, controls, key)
}

/// The Open key of the player's bindings ([`open_key`]).
fn current_open_key(e: &mut Engine, controls: u32) -> Option<u8> {
    let activate = control_key(e, controls, ACTIVATE_CONTROL);
    let reload = control_key(e, controls, RELOAD_CONTROL);
    open_key(activate, reload)
}

/// The press of `key` taken out of the controls' state: its last state made down too, so the key
/// reads as held since before and no one after this pick sees it go down (the container menu that
/// opens at once); the next poll copies it on, so it stays so while the key is held. Under that
/// menu the player's update, where Reload is read, does not run (`Main::OnIdle_UpdatePlayer`,
/// `0086f940`, takes its menu branch; crates/frame-model `player.rs`).
fn take_key_press(e: &mut Engine, controls: u32, key: u8) {
    let state = e.mem.u8(controls + KEYBOARD_STATE + u32::from(key));
    e.mem
        .set_u8(controls + KEYBOARD_LAST + u32::from(key), state);
}

/// The keys of this frame (module documentation). Only on the main thread, where the pick runs
/// (from `Main::UpdateNonRenderSafeAITasks`, or at the end of `AITaskManager::Process` with the
/// split update, both on main): the moves change the world's inventories.
fn read_keys(e: &mut Engine, target: u32) {
    if !engine::thread_host::on_main(e) {
        return;
    }
    let controls = controls(e);
    if controls == 0 {
        return;
    }
    let mut step = 0i32;
    let wheel = e.call(MOUSE_AXIS, &args![controls, 3u32]).i32();
    if wheel != 0 {
        step -= wheel.signum();
        e.mem.set_i32(controls + WHEEL_WORD, 0);
    }
    if free_key_down(e, controls, KEY_UP) {
        step -= 1;
    }
    if free_key_down(e, controls, KEY_DOWN) {
        step += 1;
    }
    if step != 0 {
        with_state(|s| {
            let (highlight, top) = scroll(s.highlight, s.top, step, s.rows.len());
            s.highlight = highlight;
            s.top = top;
        });
    }
    let open = current_open_key(e, controls);
    let presses = Presses {
        activate: e
            .call(CONTROL_STATE, &args![controls, ACTIVATE_CONTROL, 1u32])
            .i32()
            != 0,
        open: open.is_some_and(|key| key_down(e, controls, key)),
        take_all: free_key_down(e, controls, TAKE_ALL_KEY),
    };
    match action(presses) {
        Action::None => {}
        Action::Take => {
            // The player's next update activates the target from this press: not passed on.
            let row = with_state(|s| {
                s.swallow = Some((target, true));
                s.rows.get(s.highlight).cloned()
            });
            if let Some(row) = row {
                take(e, target, Some(&row));
            }
        }
        Action::TakeAll => take(e, target, None),
        Action::Open => {
            if let Some(key) = open {
                take_key_press(e, controls, key);
            }
            with_state(|s| s.swallow = None);
            hide(e);
            let player = e.global::<u32>(PLAYER);
            let count = e.call(ACTIVATE_COUNT, &args![target]).u32();
            e.call(ACTIVATE, &args![target, player, 0u32, 0u32, count]);
        }
    }
}

/// The owner a take from `target` steals from (`00704ed0`'s rule), if it is stealing.
fn stealing(e: &mut Engine, target: u32) -> Option<u32> {
    if e.vcall(target, IS_ACTOR_SLOT, &args![]).bool() {
        return None;
    }
    let owner = e.call(OWNER, &args![target]).u32();
    let player = e.global::<u32>(PLAYER);
    (owner != 0 && !e.call(IS_AN_OWNER, &args![target, player, 1u32]).bool()).then_some(owner)
}

/// Takes `row`'s stack, or every item with `None`, by the container menu's rules (module
/// documentation).
fn take(e: &mut Engine, target: u32, row: Option<&Row>) {
    let player = e.global::<u32>(PLAYER);
    let owner = stealing(e, target);
    let mut stolen = 0i32;
    let mut first = None;
    // All the copies first, then the moves, then the copies deleted: the menu too takes the items
    // of a list it made before (the walk's iterator is not used while the container changes).
    let items = collect(e, target);
    let forms: Vec<u32> = items.iter().map(|&item| e.mem.u32(item + 8)).collect();
    for index in chosen(&forms, row.map(|r| (r.form, r.stack))) {
        let item = items[index];
        if let Some(owner) = owner {
            stolen = stolen.wrapping_add(stolen_value(e, item, owner));
        }
        move_item(e, target, item, owner.is_some());
        first.get_or_insert(forms[index]);
    }
    free(e, &items);
    if let Some(form) = first {
        if row.is_some() {
            e.call(PICKUP_SOUND, &args![player, form, 1u32, 0u32]);
        } else {
            play_named_sound(e, TAKE_ALL_SOUND);
        }
    }
    if let Some(owner) = owner.filter(|_| stolen > 0) {
        e.call(
            STEAL_ALARM,
            &args![player, target, 0u32, 0u32, stolen, owner],
        );
    }
    with_state(|s| s.dirty = true);
}

/// One item from `target` to the player (`TakeAllFn`'s steps).
fn move_item(e: &mut Engine, target: u32, item: u32, stealing: bool) {
    let player = e.global::<u32>(PLAYER);
    let extra = first_extra_list(e, item);
    if extra != 0 {
        let dropper = e.call(ITEM_DROPPER, &args![extra]).u32();
        if dropper != 0
            && !e.call(FORM_FLAG_800, &args![dropper]).bool()
            && !e.call(FORM_FLAG_20, &args![dropper]).bool()
        {
            e.vcall(player, GIVE_BACK_SLOT, &args![dropper, 1u32, 0u32]);
            return;
        }
    }
    let object = e.mem.u32(item + 8);
    let count = e.mem.u32(item + 4);
    e.vcall(
        target,
        REMOVE_ITEM_SLOT,
        &args![
            object,
            extra,
            count,
            u32::from(stealing),
            0u32,
            player,
            0u32,
            0u32,
            1u32,
            0u32
        ],
    );
}

/// An item's first extra list (its list's first node's data), or 0.
fn first_extra_list(e: &Engine, item: u32) -> u32 {
    let list = e.mem.u32(item);
    if list == 0 {
        0
    } else {
        e.mem.u32(list)
    }
}

/// `ContainerMenu::GetStolenValue` (`0075e240`) for the whole stack, unowned items counting, with
/// the container's owner `owner`: the item's value times the count of each extra list owned by
/// nobody or by `owner`, and of the count no extra list holds.
fn stolen_value(e: &mut Engine, item: u32, owner: u32) -> i32 {
    let value = e.call(ITEM_VALUE, &args![item]).f64() as f32 as i32;
    let mut left = e.mem.u32(item + 4) as i32;
    let mut total = 0i32;
    let mut node = e.mem.u32(item);
    while left > 0 && node != 0 {
        let extra = e.mem.u32(node);
        if extra == 0 {
            break;
        }
        let extra_owner = e.call(EXTRA_OWNER, &args![extra]).u32();
        let stack = i32::from(e.call(EXTRA_COUNT, &args![extra]).u32() as u16 as i16);
        if extra_owner == 0 || extra_owner == owner {
            total = stack.wrapping_mul(value).wrapping_add(total);
        }
        left = left.wrapping_sub(stack);
        node = e.mem.u32(node + 4);
    }
    left.wrapping_mul(value).wrapping_add(total)
}

/// Plays a named sound once, as the container menu plays its take-all sound.
fn play_named_sound(e: &mut Engine, name: u32) {
    e.with_stack(0xc, |e, handle| {
        let audio = e.call(AUDIO_INSTANCE, &args![]).u32();
        let sound = e
            .call(AUDIO_SOUND_BY_NAME, &args![audio, handle, name, 0x121u32])
            .u32();
        e.call(SOUND_HANDLE_PLAY, &args![sound, 0u32]);
        e.call(SOUND_HANDLE_DESTRUCTOR, &args![handle]);
    });
}

/// A setting's text, or `fallback` when it has none.
fn setting_text(e: &mut Engine, setting: u32, fallback: &[u8]) -> Vec<u8> {
    if setting != 0 {
        let text = e.call(SETTING_TEXT, &args![setting]).u32();
        let text = game_string(e, text);
        if !text.is_empty() {
            return text;
        }
    }
    fallback.to_vec()
}

/// A key's name (the key names' setting text), or `fallback`.
fn key_name(e: &mut Engine, key: Option<u8>, fallback: &[u8]) -> Vec<u8> {
    match key {
        Some(key) if key < 0xee => {
            let setting = e.mem.u32(KEY_NAMES + u32::from(key) * 4);
            setting_text(e, setting, fallback)
        }
        _ => fallback.to_vec(),
    }
}

// The panel's tiles.

/// A float trait of `tile`, `None` when the tile has no such trait.
fn trait_value(e: &mut Engine, tile: u32, id: i32) -> Option<f32> {
    if tile == 0 || tile_get_value(e, Ptr::new(tile), id).is_null() {
        None
    } else {
        Some(fn_00a011b0(e, Ptr::new(tile), id))
    }
}

/// Sets a float trait of `tile` when it differs (a change makes the tile update).
fn set_value(e: &mut Engine, tile: u32, id: i32, value: f32) {
    if tile != 0 && trait_value(e, tile, id) != Some(value) {
        e.call(TILE_SET_FLOAT, &args![tile, id as u32, value, 1u32]);
    }
}

/// Sets the string trait `id` of `tile` when it differs.
fn set_text(e: &mut Engine, tile: u32, id: i32, text: &[u8]) {
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

/// A tile's color.
fn set_color(e: &mut Engine, tile: u32, color: [u8; 3]) {
    set_value(e, tile, TRAIT_RED, f32::from(color[0]));
    set_value(e, tile, TRAIT_GREEN, f32::from(color[1]));
    set_value(e, tile, TRAIT_BLUE, f32::from(color[2]));
}

/// Shows or hides a tile.
fn set_visible(e: &mut Engine, tile: u32, visible: bool) {
    set_value(e, tile, TRAIT_VISIBLE, if visible { 1.0 } else { 0.0 });
}

/// Places a text tile so that it ends at `right` (its width is its text's, from its last update).
fn place_from_right(e: &mut Engine, tile: u32, right: f32) {
    let width = trait_value(e, tile, TRAIT_WIDTH).unwrap_or(0.0);
    set_value(e, tile, TRAIT_X, (right - width).round());
}

/// The HUD and its menu tile, if the HUD is up.
fn hud_menu_tile(e: &Engine) -> Option<(u32, u32)> {
    let hud = e.global::<u32>(HUD);
    if hud == 0 {
        return None;
    }
    let tile = e.mem.u32(hud + MENU_TILE);
    (tile != 0).then_some((hud, tile))
}

/// Whether `child` is one of `parent`'s children (`Tile::xChildren`, the list at `+4`: its head
/// first; a node's next at `+0`, its tile at `+8`).
fn is_child(e: &Engine, parent: u32, child: u32) -> bool {
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

/// The panel, made under the HUD's menu tile the first time and again when the HUD was made anew.
fn panel(e: &mut Engine) -> Option<Panel> {
    let (hud, menu_tile) = hud_menu_tile(e)?;
    if let Some(panel) = current_panel() {
        if panel.hud == hud && panel.menu_tile == menu_tile && is_child(e, menu_tile, panel.root) {
            return Some(panel);
        }
    }
    let panel = build(e, hud, menu_tile);
    keep_panel(panel.clone());
    // A new panel has new icon tiles.
    with_state(|s| s.icons = Default::default());
    panel
}

/// Reads [`panel_xml`] into tiles under `menu_tile` by the steps of `Tile::ReadFile`
/// (`00a01b00`) for a text in memory: `Tile::ParseFile` with the buffer, `BuildAndNameTree`,
/// `ConnectTraitsToTree`, the storage deleted, the new tree updated under the interface's lock.
fn build(e: &mut Engine, hud: u32, menu_tile: u32) -> Option<Panel> {
    let atlas = e.global::<u32>(DEFAULT_ATLAS_NAME);
    if atlas == 0 {
        return None;
    }
    let atlas = String::from_utf8_lossy(&e.mem.cstr(atlas)).into_owned();
    let xml = panel_xml(&atlas).into_bytes();
    const PATH: &[u8] = b"nv-rs quick loot panel";
    let line_word = e.tls() + TLS_CURRENT_LINE;
    let saved_line = e.mem.u32(line_word);
    let size = xml.len() as u32 + PATH.len() as u32 + 2;
    let root = e.with_stack(size, |e, buffer| {
        let text = buffer.addr();
        let path = text + xml.len() as u32 + 1;
        e.mem.set_cstr(text, &xml);
        e.mem.set_cstr(path, PATH);
        e.mem.set_u32(line_word, 0xd);
        e.set_global(XML_ERROR_FLAG, 0u8);
        let storage = tile_parse_file(e, Ptr::new(path), Ptr::new(text), xml.len() as u32);
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
        let tile = tile_build_and_name_tree(e, Ptr::new(menu_tile), template(e));
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
    let root = root?;
    let find = |e: &mut Engine, name: &str| {
        let name = format!("nvrs_ql_{name}");
        e.with_stack(name.len() as u32 + 1, |e, buffer| {
            e.mem.set_cstr(buffer.addr(), name.as_bytes());
            tile_get_child_by_name(e, Ptr::new(root), buffer).addr()
        })
    };
    let mut panel = Panel {
        hud,
        menu_tile,
        root,
        title: find(e, "title"),
        weight: find(e, "weight"),
        highlight: find(e, "highlight"),
        track: find(e, "track"),
        thumb: find(e, "thumb"),
        take: find(e, "take"),
        take_all: find(e, "take_all"),
        open: find(e, "open"),
        ..Panel::default()
    };
    for row in 0..ROWS {
        panel.rows[row] = find(e, &format!("row{row}"));
        panel.icons[row] = find(e, &format!("icon{row}"));
        panel.names[row] = find(e, &format!("name{row}"));
    }
    Some(panel)
}

/// An icon tile showing `file`, fitted to its box keeping the image's shape: a new file is loaded
/// at zoom 100 and kept unseen until the game has read its size (`filewidth`, `fileheight`, which
/// are cleared first); then the zoom and the tile's size make the whole image fill the box's width
/// or height, centered in it.
fn show_icon(e: &mut Engine, slot: usize, tile: u32, file: &[u8], color: [u8; 3]) {
    if tile == 0 {
        return;
    }
    let (new, fitted) = with_state(|s| {
        let icon = &mut s.icons[slot];
        let new = icon.file != file;
        if new {
            icon.file = file.to_vec();
            icon.fitted = false;
        }
        (new, icon.fitted)
    });
    set_color(e, tile, color);
    if new {
        set_value(e, tile, TRAIT_ALPHA, 0.0);
        set_value(e, tile, TRAIT_ZOOM, 100.0);
        set_value(e, tile, TRAIT_FILE_WIDTH, 0.0);
        set_value(e, tile, TRAIT_FILE_HEIGHT, 0.0);
        set_text(e, tile, TRAIT_FILENAME, file);
        return;
    }
    if fitted {
        return;
    }
    let width = trait_value(e, tile, TRAIT_FILE_WIDTH).unwrap_or(0.0);
    let height = trait_value(e, tile, TRAIT_FILE_HEIGHT).unwrap_or(0.0);
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let (zoom, fitted_width, fitted_height) = icon_fit(width, height);
    set_value(e, tile, TRAIT_ZOOM, zoom);
    set_value(e, tile, TRAIT_WIDTH, fitted_width);
    set_value(e, tile, TRAIT_HEIGHT, fitted_height);
    let x = ICON_X as f32 + ((ICON_WIDTH - fitted_width) / 2.0).round();
    let y = ICON_Y as f32 + ((ICON_HEIGHT - fitted_height) / 2.0).round();
    set_value(e, tile, TRAIT_X, x);
    set_value(e, tile, TRAIT_Y, y);
    set_value(e, tile, TRAIT_ALPHA, 255.0);
    with_state(|s| s.icons[slot].fitted = true);
}

/// The zoom, width and height that show a whole image of `width` by `height` (at zoom 100) in the
/// icon's box, its shape kept.
pub fn icon_fit(width: f32, height: f32) -> (f32, f32, f32) {
    let scale = (ICON_WIDTH / width).min(ICON_HEIGHT / height);
    (
        100.0 * scale,
        (width * scale).round(),
        (height * scale).round(),
    )
}

/// Hides the HUD's own prompt while the panel shows, keeping each tile's visibility to give back
/// (a tile the HUD showed again meanwhile is kept as shown).
fn hide_hud_prompt(e: &mut Engine, hud: u32) {
    for offset in HUD_PROMPT {
        let tile = e.mem.u32(hud + offset);
        if tile == 0 {
            continue;
        }
        let visible = trait_value(e, tile, TRAIT_VISIBLE);
        if visible == Some(0.0) {
            continue;
        }
        with_state(|s| match s.hidden.iter_mut().find(|(t, _)| *t == tile) {
            Some(entry) => entry.1 = visible,
            None => s.hidden.push((tile, visible)),
        });
        set_visible(e, tile, false);
    }
}

/// The panel's title: the name the HUD made for the reference, else the reference's own.
fn title(e: &mut Engine, target: u32) -> Vec<u8> {
    let made = e.mem.cstr(HUD_TARGET_NAME);
    if !made.is_empty() {
        return made;
    }
    let name = e.call(REFERENCE_NAME, &args![target]).u32();
    game_string(e, name)
}

/// The panel filled and shown (module documentation).
fn show(e: &mut Engine) {
    let Some(panel) = panel(e) else {
        return;
    };
    let target = with_state(|s| s.target);
    let controls = controls(e);
    let (activate, open, take_all_free) = if controls == 0 {
        (None, Some(OPEN_KEY), true)
    } else {
        (
            control_key(e, controls, ACTIVATE_CONTROL),
            current_open_key(e, controls),
            !bound(e, controls, TAKE_ALL_KEY),
        )
    };
    let keys = Keys {
        activate: key_name(e, activate, b"Activate"),
        take_all: take_all_free.then(|| key_name(e, Some(TAKE_ALL_KEY), b"X")),
        open: open.map(|key| key_name(e, Some(key), b"R")),
    };
    let title = title(e, target);
    let player = e.global::<u32>(PLAYER);
    let (carried, capacity) = e.with_stack(8, |e, out| {
        e.call(PLAYER_WEIGHTS, &args![player, out.addr(), out.addr() + 4]);
        (e.mem.f32(out.addr()), e.mem.f32(out.addr() + 4))
    });
    let label = setting_text(e, WEIGHT_LABEL, b"Wg");
    let weight = weight_line(&label, carried, capacity);
    let (view, rows, stealing) = with_state(|s| {
        let view = compose(
            &title,
            &weight,
            &s.rows,
            s.highlight,
            s.top,
            &keys,
            s.stealing,
        );
        let rows: Vec<Option<Row>> = (0..ROWS)
            .map(|line| s.rows.get(s.top + line).cloned())
            .collect();
        (view, rows, s.stealing)
    });
    let color = if stealing { RED } else { WHITE };
    set_text(e, panel.title, TRAIT_STRING, &view.title);
    set_text(e, panel.weight, TRAIT_STRING, &view.weight);
    place_from_right(e, panel.weight, (PANEL_WIDTH - MARGIN) as f32);
    for (line, (label, row)) in view.lines.iter().zip(&rows).enumerate() {
        match (label, row) {
            (Some(label), Some(row)) => {
                set_visible(e, panel.rows[line], true);
                set_text(e, panel.names[line], TRAIT_STRING, label);
                set_color(e, panel.names[line], color);
                show_icon(e, line, panel.icons[line], &row.icon, color);
            }
            _ => set_visible(e, panel.rows[line], false),
        }
    }
    match view.highlight {
        Some(line) => {
            let y = (ROWS_TOP + line as u32 * ROW_HEIGHT) as f32;
            set_value(e, panel.highlight, TRAIT_Y, y);
            set_visible(e, panel.highlight, true);
        }
        None => set_visible(e, panel.highlight, false),
    }
    match view.thumb {
        Some((offset, length)) => {
            set_visible(e, panel.track, true);
            set_visible(e, panel.thumb, true);
            set_value(e, panel.thumb, TRAIT_Y, (ROWS_TOP as f32 + offset).round());
            set_value(e, panel.thumb, TRAIT_HEIGHT, length.round());
        }
        None => {
            set_visible(e, panel.track, false);
            set_visible(e, panel.thumb, false);
        }
    }
    let [take, take_all, open] = &view.footer;
    set_text(e, panel.take, TRAIT_STRING, take);
    set_color(e, panel.take, color);
    set_text(e, panel.take_all, TRAIT_STRING, take_all);
    set_visible(e, panel.take_all, !take_all.is_empty());
    let width = trait_value(e, panel.take_all, TRAIT_WIDTH).unwrap_or(0.0);
    let x = ((PANEL_WIDTH as f32 - width) / 2.0).round();
    set_value(e, panel.take_all, TRAIT_X, x);
    set_text(e, panel.open, TRAIT_STRING, open);
    set_visible(e, panel.open, !open.is_empty());
    place_from_right(e, panel.open, (PANEL_WIDTH - MARGIN) as f32);
    // The place: right of the crosshair and above it, kept on the screen.
    let screen_width = e.call(SCREEN_WIDTH, &args![]).f64() as f32;
    let screen_height = e.call(SCREEN_HEIGHT, &args![]).f64() as f32;
    let x = (screen_width / 2.0 + PANEL_RIGHT_OF_CROSSHAIR)
        .min(screen_width - PANEL_WIDTH as f32 - 8.0)
        .max(0.0);
    let y = (screen_height / 2.0 - PANEL_ABOVE_CROSSHAIR).max(0.0);
    set_value(e, panel.root, TRAIT_X, x.round());
    set_value(e, panel.root, TRAIT_Y, y.round());
    hide_hud_prompt(e, panel.hud);
    set_visible(e, panel.root, true);
    with_state(|s| s.shown = true);
}

/// The panel hidden, the HUD's prompt given back and made again ([`HUD_REFRESH`]) when the panel
/// showed, and the state let go (the swallowed Activate and the icons' files are kept).
fn hide(e: &mut Engine) {
    let (was, hidden) = with_state(|s| {
        let was = s.shown;
        let hidden = std::mem::take(&mut s.hidden);
        let swallow = s.swallow;
        let icons = std::mem::take(&mut s.icons);
        *s = State::default();
        s.swallow = swallow;
        s.icons = icons;
        (was, hidden)
    });
    if !was {
        return;
    }
    if let Some(panel) = current_panel() {
        if hud_menu_tile(e).is_some_and(|(_, menu_tile)| is_child(e, menu_tile, panel.root)) {
            set_visible(e, panel.root, false);
        }
    }
    for (tile, visible) in hidden {
        set_value(e, tile, TRAIT_VISIBLE, visible.unwrap_or(1.0));
    }
    e.call(HUD_REFRESH, &args![]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, count: i32) -> Row {
        Row {
            form: 0x100,
            stack: 0,
            count,
            name: name.as_bytes().to_vec(),
            icon: b"Interface\\Icons\\PipboyImages\\Items\\items_stimpak.dds".to_vec(),
        }
    }

    fn keys(take_all: bool) -> Keys {
        Keys {
            activate: b"E".to_vec(),
            take_all: take_all.then(|| b"X".to_vec()),
            open: Some(b"R".to_vec()),
        }
    }

    fn text(bytes: &[u8]) -> &str {
        std::str::from_utf8(bytes).unwrap()
    }

    #[test]
    fn the_panel_has_the_name_the_weight_the_rows_and_the_keys() {
        let rows = [
            row("Spiked Knuckles", 1),
            row("Raider Blastmaster Armor", 1),
            row("Case, .357 Magnum", 13),
        ];
        let weight = weight_line(b"Wg", 264.6, 401.0);
        assert_eq!(text(&weight), "Wg 265/401");
        let view = compose(b"Raider", &weight, &rows, 1, 0, &keys(true), false);
        assert_eq!(text(&view.title), "Raider");
        assert_eq!(text(&view.weight), "Wg 265/401");
        let lines: Vec<Option<&str>> = view.lines.iter().map(|l| l.as_deref().map(text)).collect();
        assert_eq!(
            lines,
            [
                Some("Spiked Knuckles"),
                Some("Raider Blastmaster Armor"),
                Some("Case, .357 Magnum (13)"),
                None,
                None
            ]
        );
        assert_eq!(view.highlight, Some(1));
        assert_eq!(view.thumb, None, "every row shows: no scroll bar");
        let footer: Vec<&str> = view.footer.iter().map(|p| text(p)).collect();
        assert_eq!(footer, ["E) Take", "X) Take All", "R) Open"]);
    }

    #[test]
    fn the_footer_names_the_working_keys_and_says_steal_when_it_is() {
        let mut keys = keys(false);
        keys.activate = b"F".to_vec();
        keys.open = Some(b"T".to_vec());
        let footer = prompts(&keys, true);
        assert_eq!(text(&footer[0]), "F) Steal");
        assert!(footer[1].is_empty(), "a bound X: no Take All");
        assert_eq!(text(&footer[2]), "T) Open");
        keys.open = None;
        assert!(
            prompts(&keys, false)[2].is_empty(),
            "no Open key: no prompt"
        );
    }

    #[test]
    fn a_long_list_scrolls_with_a_thumb_and_long_names_are_cut() {
        let rows: Vec<Row> = (0..10).map(|i| row(&format!("Item {i}"), 1)).collect();
        let view = compose(b"Footlocker", b"Wg 1/2", &rows, 0, 0, &keys(true), false);
        assert_eq!(view.lines.len(), ROWS);
        assert_eq!(view.lines[0].as_deref(), Some(b"Item 0" as &[u8]));
        assert_eq!(view.lines[ROWS - 1].as_deref(), Some(b"Item 4" as &[u8]));
        let (offset, length) = view.thumb.unwrap();
        assert_eq!(offset, 0.0);
        assert_eq!(length, (ROWS as u32 * ROW_HEIGHT) as f32 / 2.0);
        let view = compose(b"Footlocker", b"Wg 1/2", &rows, 9, 5, &keys(true), false);
        assert_eq!(view.highlight, Some(4));
        assert_eq!(view.lines[4].as_deref(), Some(b"Item 9" as &[u8]));
        let (offset, length) = view.thumb.unwrap();
        assert_eq!(
            offset + length,
            (ROWS as u32 * ROW_HEIGHT) as f32,
            "at the end"
        );
        // A name longer than a row keeps its count.
        let long = row("Weapon Repair Kit of the Very Long Name Department", 12);
        let label = row_label(&long);
        assert_eq!(label.len(), ROW_CHARACTERS);
        assert!(text(&label).ends_with("... (12)"), "{}", text(&label));
        let title = fit(b"A Very Long Footlocker Name Indeed", HEADER_CHARACTERS);
        assert_eq!(title.len(), HEADER_CHARACTERS);
        assert!(text(&title).starts_with("A Very Long Foot") && text(&title).ends_with("..."));
    }

    #[test]
    fn the_highlight_stays_in_the_list_and_in_view() {
        assert_eq!(scroll(0, 0, -1, 10), (0, 0));
        assert_eq!(scroll(0, 0, 1, 10), (1, 0));
        assert_eq!(scroll(4, 0, 1, 10), (5, 1));
        assert_eq!(scroll(9, 5, 5, 10), (9, 5));
        assert_eq!(scroll(3, 3, -1, 10), (2, 2));
        // The list got shorter: the highlight comes back into it, and the lines fill.
        assert_eq!(scroll(9, 5, 0, 4), (3, 0));
        assert_eq!(scroll(5, 2, 0, 0), (0, 0));
    }

    #[test]
    fn r_opens_unless_it_is_activate() {
        // The defaults: Activate E (0x12), Reload R.
        assert_eq!(open_key(Some(0x12), Some(OPEN_KEY)), Some(OPEN_KEY));
        // Reload moved elsewhere: R still opens.
        assert_eq!(open_key(Some(0x12), Some(0x21)), Some(OPEN_KEY));
        // R is Activate: Activate keeps it, Open takes Reload's key.
        assert_eq!(open_key(Some(OPEN_KEY), Some(0x12)), Some(0x12));
        // ... and there is none when Reload has no other key.
        assert_eq!(open_key(Some(OPEN_KEY), None), None);
        assert_eq!(open_key(Some(OPEN_KEY), Some(OPEN_KEY)), None);
        // Activate on the mouse: R opens.
        assert_eq!(open_key(None, Some(OPEN_KEY)), Some(OPEN_KEY));
    }

    #[test]
    fn activate_takes_on_its_press_and_holding_it_never_opens() {
        let press = |activate, open, take_all| {
            action(Presses {
                activate,
                open,
                take_all,
            })
        };
        // A tap takes; a held Activate makes no press after its first frame, so it neither takes
        // again nor opens, however long it is held.
        assert_eq!(press(true, false, false), Action::Take);
        for _frame in 0..600 {
            assert_eq!(press(false, false, false), Action::None);
        }
        // R opens, X takes all.
        assert_eq!(press(false, true, false), Action::Open);
        assert_eq!(press(false, false, true), Action::TakeAll);
        assert_eq!(press(true, true, false), Action::Open);
    }

    #[test]
    fn an_icon_fits_its_box_and_keeps_its_shape() {
        // A 2:1 inventory image fills the box (which is 2:1 too).
        let (zoom, width, height) = icon_fit(256.0, 128.0);
        assert_eq!((width, height), (ICON_WIDTH, ICON_HEIGHT));
        assert_eq!(zoom, 100.0 * ICON_WIDTH / 256.0);
        // A square one fills its height.
        let (_, width, height) = icon_fit(128.0, 128.0);
        assert_eq!((width, height), (ICON_HEIGHT, ICON_HEIGHT));
        // A wide one fills its width.
        let (_, width, height) = icon_fit(512.0, 128.0);
        assert_eq!((width, height), (ICON_WIDTH, (ICON_WIDTH / 4.0).round()));
    }

    #[test]
    fn the_panel_xml_has_its_tiles_and_the_atlas() {
        let xml = panel_xml("Interface\\InterfaceShared.tai");
        assert!(xml.starts_with("<rect name=\"nvrs_ql_panel\">"));
        assert!(xml.trim_end().ends_with("</rect>"));
        for name in [
            "title",
            "weight",
            "rule",
            "highlight",
            "marker",
            "track",
            "thumb",
            "footer_rule",
            "take",
            "take_all",
            "open",
        ] {
            assert!(xml.contains(&format!("name=\"nvrs_ql_{name}\"")), "{name}");
        }
        for row in 0..ROWS {
            for part in ["row", "icon", "name"] {
                assert!(
                    xml.contains(&format!("name=\"nvrs_ql_{part}{row}\"")),
                    "{part}{row}"
                );
            }
        }
        assert!(xml.contains("<texatlas>Interface\\InterfaceShared.tai</texatlas>"));
        // The box is mostly see-through (Patrick: "so I can somewhat see through it"), the words
        // white.
        const _: () = assert!(BACK_ALPHA <= 96);
        assert!(xml.contains(&format!(
            "name=\"nvrs_ql_back\"><filename>solid.dds</filename>\
             <texatlas>Interface\\InterfaceShared.tai</texatlas><x>0</x><y>0</y>\
             <width>{PANEL_WIDTH}</width><height>{PANEL_HEIGHT}</height><red>0</red><green>0</green>\
             <blue>0</blue><alpha>{BACK_ALPHA}</alpha>"
        )));
        assert!(xml.contains("<text name=\"nvrs_ql_take\"><x>12</x><y>"));
        assert_eq!(
            xml.matches("<font>3</font><red>255</red><green>255</green><blue>255</blue>")
                .count(),
            xml.matches("<text ").count(),
            "every text starts white"
        );
        assert!(xml.contains(&format!("<font>{FONT}</font>")));
        // Balanced: every rect, image and text closes.
        for tag in ["rect", "image", "text"] {
            assert_eq!(
                xml.matches(&format!("<{tag} ")).count(),
                xml.matches(&format!("</{tag}>")).count(),
                "{tag}"
            );
        }
        // The footer and the panel's height: the panel ends a line under the footer's text.
        assert!(xml.contains(&format!("<height>{PANEL_HEIGHT}</height>")));
        assert!(xml.contains(&format!("<y>{FOOTER_Y}</y>")));
    }

    #[test]
    fn a_row_takes_its_own_stack_and_take_all_takes_every_one() {
        let forms = [0xa, 0xb, 0xa, 0xc];
        assert_eq!(chosen(&forms, Some((0xa, 0))), [0]);
        assert_eq!(
            chosen(&forms, Some((0xa, 1))),
            [2],
            "the second stack of the same object"
        );
        assert_eq!(chosen(&forms, Some((0xc, 0))), [3]);
        assert!(
            chosen(&forms, Some((0xd, 0))).is_empty(),
            "gone meanwhile: nothing"
        );
        assert!(chosen(&forms, Some((0xb, 1))).is_empty());
        assert_eq!(chosen(&forms, None), [0, 1, 2, 3]);
    }

    /// An engine whose one item copy is at 0x1000: its extra lists from 0x1100 (owned by 0x900,
    /// 3 of the 5; then one unowned list of 1), so the stolen value counts every unit.
    #[test]
    fn the_stolen_value_counts_owned_and_unowned_stacks() {
        let mut e = Engine::new();
        e.mem.map(0x1000, 0x1000);
        let item = 0x1000;
        e.mem.set_u32(item, 0x1100);
        e.mem.set_u32(item + 4, 5);
        // Two nodes: (extra 0x1200, next 0x1108), (extra 0x1300, next 0).
        e.mem.set_u32(0x1100, 0x1200);
        e.mem.set_u32(0x1104, 0x1108);
        e.mem.set_u32(0x1108, 0x1300);
        e.mem.set_u32(0x110c, 0);
        e.register(ITEM_VALUE, |_, _| Ret {
            st0: 10.0,
            ..Ret::default()
        });
        e.register(EXTRA_OWNER, |_, a| Ret {
            eax: if a[0] == 0x1200 { 0x900 } else { 0 },
            ..Ret::default()
        });
        e.register(EXTRA_COUNT, |_, a| Ret {
            eax: if a[0] == 0x1200 { 3 } else { 1 },
            ..Ret::default()
        });
        // 3 owned by the owner, 1 unowned, 1 in no list: all five count.
        assert_eq!(stolen_value(&mut e, item, 0x900), 50);
        // Owned by someone else: those 3 don't.
        assert_eq!(stolen_value(&mut e, item, 0x901), 20);
    }
}
