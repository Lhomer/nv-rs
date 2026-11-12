//! Quick loot (`NV_QUICK_LOOT`), a row of the `fixes` preset ([`crate::fixes`], docs/FIXES.md):
//! **not 1:1 with FalloutNV.exe**, an nv-rs addition the player asked for (Patrick, 2026-11-12: "a
//! quick loot system like in Fallout 4"); in the `vanilla` preset, or with `bQuickLoot=0`, Activate
//! opens a container or a corpse as the original does and nothing here runs.
//!
//! **What the player sees.** Looking at a container or a corpse (the HUD's target type 2,
//! `TESObjectREFR::GetTargetType` `00579280`) within the game's activation reach, the HUD's own
//! prompt (the action line, "Search", with the name under it) gets the items listed under the name,
//! in the same tile and font (the name tile, `HUDMainMenu +0xa8`): one line a stack, "name (count)",
//! the highlighted one marked `>`, up to [`ROWS`] lines with a "more" line when there are more, and
//! a last line naming the keys. The world goes on: it is the HUD, not a menu. The list goes when the
//! player looks away, moves out of reach, or any menu opens; it comes back when they look again.
//!
//! **What the player does.** Read through the game's controls (`Controls`, `00a24660` and the key
//! and mouse queries), so the player's own bindings count:
//! - the wheel (`00a239e0` axis 3; then cleared, so it doesn't also zoom the camera,
//!   `PlayerCharacter`'s `009445b0`) and the Up and Down arrow keys move the highlight;
//! - a tap of Activate (control 5) takes the highlighted stack, whole;
//! - holding Activate [`HOLD_SECONDS`] opens the original menu, through the original Activate
//!   (the container's own lid, sound, menu, and everything that menu does);
//! - X takes everything ([`TAKE_ALL_KEY`]).
//!
//! A key the player has bound to a control is not used here: the control keeps it (the key line says
//! which keys work).
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
//! acts on it, and the wheel cleared here is not read by it; and in front of `TESObjectREFR::Activate`
//! (`00573170`): the one Activate the player's update makes from a press seen here is not passed
//! on, so the press can be a tap or a hold. Every other Activate goes through as it is.

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::unplaced::run_0070bbe0::fn_0070bc20;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The crosshair pick (`InterfaceManager`, `0070bc20`) and `TESObjectREFR::Activate` (`00573170`).
pub const PICK: u32 = 0x0070_bc20;
pub const ACTIVATE: u32 = 0x0057_3170;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "quick loot",
    env: "NV_QUICK_LOOT",
    addresses: &[PICK, ACTIVATE],
    summary: "looking at a container or a corpse lists its items on the HUD; Activate takes one, X takes all, holding Activate opens the original menu",
    install,
};

/// Lines of items shown at once.
pub const ROWS: usize = 7;
/// How long Activate is held to open the original menu.
pub const HOLD_SECONDS: f32 = 0.45;
/// The Take All key (`DIK_X`) and the arrows (`DIK_UP`, `DIK_DOWN`).
pub const TAKE_ALL_KEY: u8 = 0x2d;
const KEY_UP: u8 = 0xc8;
const KEY_DOWN: u8 = 0xd0;
/// Frames between two readings of a container's items while it is looked at (another hand may
/// change them).
const REREAD_FRAMES: u32 = 30;

// The game's words and functions.
const PLAYER: u32 = 0x011d_ea3c;
const HUD: u32 = 0x011d_96c0;
/// `HUDMainMenu`'s name tile and the string trait.
const NAME_TILE: u32 = 0xa8;
const TRAIT_STRING: u32 = 0xfc4;
const TILE_GET_STRING: u32 = 0x00a0_11f0;
const TILE_SET_STRING: u32 = 0x00a0_1350;
/// `HUDMainMenu::RefreshInfoForRef` (`00778b10`): the prompt made again for the last reference.
const HUD_REFRESH: u32 = 0x0077_8b10;
/// In menu mode (`007023a0`, the manager), the activate reference (`00703350`).
const MENU_MODE: u32 = 0x0070_23a0;
const ACTIVATE_REF: u32 = 0x0070_3350;
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
/// (`004be2d0`) and delete (`004459e0`).
const ITEM_TOTAL: u32 = 0x0057_5590;
const INVENTORY_START: u32 = 0x0057_5510;
const NEXT_ITEM: u32 = 0x004c_a330;
const ITERATOR_DELETE: u32 = 0x004b_f630;
const ITEM_FULL_NAME: u32 = 0x004b_e2d0;
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
/// Sounds: the actor's pick-up sound (`008aded0`), and a named sound (`BSAudio::QInstance`,
/// `GetSoundHandleByName`, `Play`, the handle's destructor) for the menu's take-all sound.
const PICKUP_SOUND: u32 = 0x008a_ded0;
const AUDIO_INSTANCE: u32 = 0x0045_3a70;
const AUDIO_SOUND_BY_NAME: u32 = 0x00ad_7550;
const SOUND_HANDLE_PLAY: u32 = 0x00ad_8830;
const SOUND_HANDLE_DESTRUCTOR: u32 = 0x0048_3710;
const TAKE_ALL_SOUND: u32 = 0x0107_22c0;
/// The controls: their object (`00877720` of the word at `011dea0c`), a control's state
/// (`00a24660`: mode 0 held, 1 went down), a key's (`00a24180`), a mouse axis (`00a239e0`, 3 the
/// wheel) and the wheel's word (`+0x1b2c`), a control's bindings (`00a24f10`), the key a control is
/// bound to (`00703be0`, device 0 the keyboard), the key names (`011d52f0`, settings) and a
/// setting's text (`00403df0`).
const CONTROLS_OWNER: u32 = 0x011d_ea0c;
const CONTROLS_GET: u32 = 0x0087_7720;
const CONTROL_STATE: u32 = 0x00a2_4660;
const KEY_STATE: u32 = 0x00a2_4180;
const MOUSE_AXIS: u32 = 0x00a2_39e0;
const WHEEL_WORD: u32 = 0x1b2c;
const BINDINGS: u32 = 0x00a2_4f10;
const KEY_OF_CONTROL: u32 = 0x0070_3be0;
const KEY_NAMES: u32 = 0x011d_52f0;
const SETTING_TEXT: u32 = 0x0040_3df0;
const ACTIVATE_CONTROL: u32 = 5;
const CONTROL_COUNT: u32 = 0x1c;

/// One line of the list: the object, which of its stacks (the walk gives one item a stack), its
/// count and its name.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub form: u32,
    pub stack: u32,
    pub count: i32,
    pub name: Vec<u8>,
}

/// What the popup keeps between frames.
#[derive(Debug, Default)]
struct State {
    /// The reference listed, its rows, the name the HUD gave it, the highlight and the first line
    /// shown.
    target: u32,
    rows: Vec<Row>,
    name: Vec<u8>,
    highlight: usize,
    top: usize,
    /// Frames since the rows were read, and whether they must be read again.
    age: u32,
    dirty: bool,
    /// Whether the HUD's name tile has the list, and the text it was given.
    shown: bool,
    text: Vec<u8>,
    /// An Activate press seen here: when, and the reference whose Activate is not passed on.
    press: Option<Instant>,
    swallow: Option<u32>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(State::default))
}

/// Registers the pick and `Activate`.
pub fn install(e: &mut Engine) {
    e.register(PICK, |e, a| {
        fn_0070bc20(e, Ptr::new(a[0]));
        after_pick(e, a[0]);
        Ret::default()
    });
    e.register(ACTIVATE, |e, a| {
        let swallowed = with_state(|s| {
            if a[1] != 0 && a[1] == e.global::<u32>(PLAYER) && s.swallow == Some(a[0]) {
                s.swallow = None;
                true
            } else {
                false
            }
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

// The parts that need no game: the text, the highlight, a press.

/// What a press of Activate comes to: still undecided, a tap (take), or a hold (open the menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Waiting,
    Take,
    Open,
}

/// A press `held` (still down) after `elapsed`.
pub fn press(held: bool, elapsed: Duration) -> Press {
    if !held {
        Press::Take
    } else if elapsed.as_secs_f32() >= HOLD_SECONDS {
        Press::Open
    } else {
        Press::Waiting
    }
}

/// The highlight moved by `step` lines inside `count` rows, and the first line shown so that it
/// is among the [`ROWS`] shown.
pub fn scroll(highlight: usize, top: usize, step: i32, count: usize) -> (usize, usize) {
    if count == 0 {
        return (0, 0);
    }
    let highlight = (highlight as i64 + i64::from(step)).clamp(0, count as i64 - 1) as usize;
    let mut top = top.min(count.saturating_sub(1));
    if highlight < top {
        top = highlight;
    } else if highlight >= top + ROWS {
        top = highlight + 1 - ROWS;
    }
    (highlight, top)
}

/// The keys' names for the last line: Activate's, and Take All's when it is free.
pub struct Keys {
    pub activate: Vec<u8>,
    pub take_all: Option<Vec<u8>>,
}

/// The name tile's text: the name, the rows shown (the highlight marked `>`), a line for rows
/// below, and the keys.
pub fn compose(name: &[u8], rows: &[Row], highlight: usize, top: usize, keys: &Keys) -> Vec<u8> {
    let mut text = name.to_vec();
    for (index, row) in rows.iter().enumerate().skip(top).take(ROWS) {
        text.push(b'\n');
        text.extend_from_slice(if index == highlight { b"> " } else { b"  " });
        text.extend_from_slice(&row.name);
        if row.count > 1 {
            text.extend_from_slice(format!(" ({})", row.count).as_bytes());
        }
    }
    let below = rows.len().saturating_sub(top + ROWS);
    if below > 0 {
        text.extend_from_slice(format!("\n  ({below} more)").as_bytes());
    }
    text.push(b'\n');
    text.extend_from_slice(&keys.activate);
    text.extend_from_slice(b") Take   ");
    if let Some(take_all) = &keys.take_all {
        text.extend_from_slice(take_all);
        text.extend_from_slice(b") Take All   ");
    }
    text.extend_from_slice(b"Hold ");
    text.extend_from_slice(&keys.activate);
    text.extend_from_slice(b") Open");
    text
}

// The game.

/// After each pick: the list kept, shown, hidden, and the keys read (module documentation).
fn after_pick(e: &mut Engine, manager: u32) {
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
        if changed {
            s.target = target;
            s.highlight = 0;
            s.top = 0;
            s.press = None;
            s.swallow = None;
        }
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
        with_state(|s| {
            s.rows = rows;
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

/// `target`'s rows (the stacks [`collect`] gives, numbered within each object).
fn rows(e: &mut Engine, target: u32) -> Vec<Row> {
    let items = collect(e, target);
    let mut rows: Vec<Row> = Vec::new();
    for &item in &items {
        let form = e.mem.u32(item + 8);
        let stack = rows.iter().filter(|r| r.form == form).count() as u32;
        let name = e.call(ITEM_FULL_NAME, &args![item]).u32();
        let name = if name == 0 {
            Vec::new()
        } else {
            e.mem.cstr(name)
        };
        rows.push(Row {
            form,
            stack,
            count: e.mem.u32(item + 4) as i32,
            name,
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

/// Whether keyboard key `key` is bound to any of the game's controls.
fn bound(e: &mut Engine, controls: u32, key: u8) -> bool {
    (0..CONTROL_COUNT).any(|control| {
        let packed = e.call(BINDINGS, &args![controls, control]).u32();
        (packed >> 16) as u8 == key
    })
}

/// A free key's state `mode` (a bound key is the control's).
fn key(e: &mut Engine, controls: u32, key: u8, mode: u32) -> bool {
    !bound(e, controls, key)
        && e.call(KEY_STATE, &args![controls, u32::from(key), mode])
            .bool()
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
    if key(e, controls, KEY_UP, 1) {
        step -= 1;
    }
    if key(e, controls, KEY_DOWN, 1) {
        step += 1;
    }
    if step != 0 {
        with_state(|s| {
            let (highlight, top) = scroll(s.highlight, s.top, step, s.rows.len());
            s.highlight = highlight;
            s.top = top;
        });
    }
    if key(e, controls, TAKE_ALL_KEY, 1) {
        take(e, target, None);
        return;
    }
    let down = e
        .call(CONTROL_STATE, &args![controls, ACTIVATE_CONTROL, 1u32])
        .i32()
        != 0;
    let held = e
        .call(CONTROL_STATE, &args![controls, ACTIVATE_CONTROL, 0u32])
        .i32()
        != 0;
    let now = Instant::now();
    let decision = with_state(|s| {
        if s.press.is_none() && down {
            s.press = Some(now);
            s.swallow = Some(target);
        }
        s.press.map(|start| press(held, now - start))
    });
    match decision {
        Some(Press::Take) => {
            let row = with_state(|s| {
                s.press = None;
                s.swallow = None;
                s.rows.get(s.highlight).cloned()
            });
            if let Some(row) = row {
                take(e, target, Some(&row));
            }
        }
        Some(Press::Open) => {
            with_state(|s| {
                s.press = None;
                s.swallow = None;
            });
            hide(e);
            let player = e.global::<u32>(PLAYER);
            let count = e.call(0x0077_8930, &args![target]).u32();
            e.call(ACTIVATE, &args![target, player, 0u32, 0u32, count]);
        }
        _ => {}
    }
}

/// Takes `row`'s stack, or every item with `None`, by the container menu's rules (module
/// documentation).
fn take(e: &mut Engine, target: u32, row: Option<&Row>) {
    let player = e.global::<u32>(PLAYER);
    let actor = e.vcall(target, IS_ACTOR_SLOT, &args![]).bool();
    let owner = if actor {
        0
    } else {
        e.call(OWNER, &args![target]).u32()
    };
    let stealing = owner != 0 && !e.call(IS_AN_OWNER, &args![target, player, 1u32]).bool();
    let mut stolen = 0i32;
    let mut first = None;
    // All the copies first, then the moves, then the copies deleted: the menu too takes the items
    // of a list it made before (the walk's iterator is not used while the container changes).
    let items = collect(e, target);
    let forms: Vec<u32> = items.iter().map(|&item| e.mem.u32(item + 8)).collect();
    for index in chosen(&forms, row.map(|r| (r.form, r.stack))) {
        let item = items[index];
        if stealing {
            stolen = stolen.wrapping_add(stolen_value(e, item, owner));
        }
        move_item(e, target, item, stealing);
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
    if stealing && stolen > 0 {
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

/// A key's name (the key names' setting text), or `fallback`.
fn key_name(e: &mut Engine, key: u32, fallback: &[u8]) -> Vec<u8> {
    if key < 0xee {
        let setting = e.mem.u32(KEY_NAMES + key * 4);
        if setting != 0 {
            let text = e.call(SETTING_TEXT, &args![setting]).u32();
            if text != 0 {
                let name = e.mem.cstr(text);
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    fallback.to_vec()
}

/// The HUD's name tile.
fn name_tile(e: &Engine) -> u32 {
    let hud = e.global::<u32>(HUD);
    if hud == 0 {
        0
    } else {
        e.mem.u32(hud + NAME_TILE)
    }
}

/// The list into the name tile (the HUD's name kept first, read when the list first shows).
fn show(e: &mut Engine) {
    let tile = name_tile(e);
    if tile == 0 {
        return;
    }
    if !with_state(|s| s.shown) {
        let text = e.call(TILE_GET_STRING, &args![tile, TRAIT_STRING]).u32();
        let name = if text == 0 {
            Vec::new()
        } else {
            e.mem.cstr(text)
        };
        with_state(|s| s.name = name);
    }
    let controls = controls(e);
    let activate = e
        .call(KEY_OF_CONTROL, &args![controls, ACTIVATE_CONTROL, 0u32])
        .u32()
        & 0xff;
    let keys = Keys {
        activate: key_name(e, activate, b"Activate"),
        take_all: if controls != 0 && bound(e, controls, TAKE_ALL_KEY) {
            None
        } else {
            Some(key_name(e, u32::from(TAKE_ALL_KEY), b"X"))
        },
    };
    let text = with_state(|s| compose(&s.name, &s.rows, s.highlight, s.top, &keys));
    if with_state(|s| s.shown && s.text == text) {
        return;
    }
    e.with_stack(text.len() as u32 + 1, |e, buffer| {
        e.mem.set_cstr(buffer.addr(), &text);
        e.call(TILE_SET_STRING, &args![tile, TRAIT_STRING, buffer, 1u32]);
    });
    with_state(|s| {
        s.shown = true;
        s.text = text;
    });
}

/// The HUD's own prompt again ([`HUD_REFRESH`]) when the list was shown, and the state let go.
fn hide(e: &mut Engine) {
    let was = with_state(|s| {
        let was = s.shown;
        *s = State::default();
        was
    });
    if was {
        e.call(HUD_REFRESH, &args![]);
    }
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
        }
    }

    fn keys(take_all: bool) -> Keys {
        Keys {
            activate: b"E".to_vec(),
            take_all: take_all.then(|| b"X".to_vec()),
        }
    }

    #[test]
    fn the_list_names_the_stacks_marks_the_highlight_and_names_the_keys() {
        let rows = [row("Stimpak", 2), row("10mm Round", 24), row("Pencil", 1)];
        let text = compose(b"Desk", &rows, 1, 0, &keys(true));
        assert_eq!(
            String::from_utf8(text).unwrap(),
            "Desk\n  Stimpak (2)\n> 10mm Round (24)\n  Pencil\nE) Take   X) Take All   Hold E) Open"
        );
        // A bound Take All key is not offered.
        let text = compose(b"Desk", &rows[..1], 0, 0, &keys(false));
        assert_eq!(
            String::from_utf8(text).unwrap(),
            "Desk\n> Stimpak (2)\nE) Take   Hold E) Open"
        );
    }

    #[test]
    fn a_long_list_shows_rows_lines_and_how_many_more() {
        let rows: Vec<Row> = (0..10).map(|i| row(&format!("Item {i}"), 1)).collect();
        let text = String::from_utf8(compose(b"Footlocker", &rows, 0, 0, &keys(true))).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1 + ROWS + 1 + 1);
        assert_eq!(lines[1], "> Item 0");
        assert_eq!(lines[ROWS], "  Item 6");
        assert_eq!(lines[ROWS + 1], "  (3 more)");
        let text = String::from_utf8(compose(b"Footlocker", &rows, 9, 3, &keys(true))).unwrap();
        assert!(text.contains("> Item 9") && !text.contains("more") && !text.contains("Item 2\n"));
    }

    #[test]
    fn the_highlight_stays_in_the_list_and_in_view() {
        assert_eq!(scroll(0, 0, -1, 10), (0, 0));
        assert_eq!(scroll(0, 0, 1, 10), (1, 0));
        assert_eq!(scroll(6, 0, 1, 10), (7, 1));
        assert_eq!(scroll(9, 3, 5, 10), (9, 3));
        assert_eq!(scroll(3, 3, -1, 10), (2, 2));
        // The list got shorter: the highlight comes back into it.
        assert_eq!(scroll(9, 3, 0, 4), (3, 3));
        assert_eq!(scroll(5, 2, 0, 0), (0, 0));
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

    #[test]
    fn a_tap_takes_and_a_hold_opens_the_menu() {
        assert_eq!(press(false, Duration::from_millis(80)), Press::Take);
        assert_eq!(press(true, Duration::from_millis(80)), Press::Waiting);
        assert_eq!(press(true, Duration::from_millis(450)), Press::Open);
        assert_eq!(press(false, Duration::from_millis(900)), Press::Take);
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
