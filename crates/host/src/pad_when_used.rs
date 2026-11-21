//! The keyboard and mouse beside a pad (`NV_PAD_WHEN_USED`), a row of the `fixes` preset
//! ([`crate::fixes`], docs/FIXES.md): **not 1:1 with FalloutNV.exe**, an nv-rs change the player
//! asked for (Patrick, 2026-11-21: "Keyboard and mouse must ALWAYS work, whether or not a pad is
//! connected and whatever bDisable360Controller says; a pad can work alongside them, but must never
//! lock out the keyboard. Esc must always open the menu"; "Start in keyboard-and-mouse mode by
//! default; only switch the on-screen prompts to the pad when the pad is actually used"). In the
//! `vanilla` preset, or with `bPadWhenUsed=0`, the game reads the pad as the original does.
//!
//! **What the original does with a pad.** Every frame `Controls::Poll` (`00a23010`) asks
//! `XInputGetState(0)` and sets the "pad connected" byte (`011f35c8`) when it answers and the input
//! object's switch (`Controls` +0) is clear. While that byte is set the game's control reader
//! (`00a24660`, `Controls::GetUserAction`) and its key test (`00a24280`) read only the pad's
//! bindings: the keyboard and the mouse are not read, and the fixed control `0x1c` (the menu, Esc)
//! is the pad's Start alone. `InterfaceManager::Idle` (`0070c4a0`) asks `XInputGetState(0)` too
//! and keeps the answer in `bHas360Controller` (`011d8a84`), which the prompts, the cursor and the
//! menus read (`004b71d0`: that byte and `bDisable360Controller` off); when it changes, it shows a
//! message box about the pad coming or going and refreshes the interface (`00719630`). So a pad
//! that Windows reports (a real one, Steam's or a headset's virtual pad) took the keyboard away.
//!
//! **Why `bDisable360Controller=1` did not help in the test room.** The setting
//! (`[Interface] bDisable360Controller`, the INI pref setting at `011d8c4c`, its value at +4) reaches
//! the input object's switch only when the start menu makes its Settings options: the "360
//! Controller" option's change function (`007d2fc0`), reading, copies the setting into `Controls`
//! +0 (`005407b0`), and choosing it in the menu does the same (`00719740`). Nothing else writes
//! that byte. The test room (`--room`) goes into play without the start menu ever making its
//! options, so the byte stayed 0, `Controls::Poll` kept counting the pad, and the keyboard and Esc
//! stayed dead while the menus (which read the setting itself through `004b71d0`) took the mouse.
//! In a play run the start menu had made them, which is why the in-game switch worked there.
//!
//! **What this row does.** The game's `XInputGetState` thunk (`009f996e`, every game read of the
//! pad goes through it) is replaced by one that asks the same import slot (`00fdf394`; JIP LN
//! NVSE's hook, when on, wraps that slot and still sees every read) and then answers "connected"
//! only while the pad is the device in use:
//! - the pad becomes the device in use when its state changes (a new packet number) to a state past
//!   XInput's dead zones (a button, a trigger past 30, a stick past 7849 or 8689);
//! - the keyboard and mouse become it again when a key or mouse button goes down or the mouse moves
//!   (`engine_platform::dinput::keyboard_mouse_uses`, counted as the platform hands the window's
//!   input to the game's DirectInput, before the frame's poll reads it, so the very key that switches
//!   back is read by the keyboard path: the first Esc opens the menu); in the same frame as a pad
//!   change they win;
//! - a run starts with the keyboard and mouse in use, and a pad that is pulled out leaves them in use;
//! - with `bDisable360Controller` set (read from the setting itself, so a start without the start
//!   menu has it too) the pad is never in use.
//!
//! The answer is the pad's own result while it is in use and `ERROR_DEVICE_NOT_CONNECTED`
//! otherwise; the pad's state is passed through either way (so its Start still opens the menu
//! beside the keyboard, through `00a24660`'s fixed control). When `Idle`'s read (its state block
//! `011d8a6c`) changes its answer only because the device in use changed (the pad was there before
//! and is still there), the switch is made the way the game makes a change, without its message box:
//! `bHas360Controller` and the last value `Idle` compares with (`011d8c70`) both take the new answer
//! and the interface is refreshed with it (`00719630` on the manager, `011d8a80`), so the prompts and
//! the cursor follow the device and `Idle` sees nothing to announce. A pad plugged in or pulled out
//! while it is the device in use still gets the game's own message.

use crate::fixes::Fix;
use engine::prelude::*;
use engine_platform::xinput::{PadState, ERROR_DEVICE_NOT_CONNECTED, ERROR_SUCCESS};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicBool, AtomicU32};

/// The game's `XInputGetState` thunk (`JMP [00fdf394]`), which this row replaces.
pub const XINPUT_THUNK: u32 = 0x009f_996e;
/// The import slot the thunk jumps through (the platform's `XInputGetState`; JIP wraps it).
const XINPUT_SLOT: u32 = 0x00fd_f394;
/// `InterfaceManager::Idle`'s state block for its read of the pad.
const IDLE_STATE: u32 = 0x011d_8a6c;
/// `bHas360Controller`, and `Idle`'s `static bool lastHasController` with its init guard.
const HAS_360_CONTROLLER: u32 = 0x011d_8a84;
const LAST_HAS_CONTROLLER: u32 = 0x011d_8c70;
const LAST_HAS_CONTROLLER_GUARD: u32 = 0x011d_8c74;
/// The interface manager (`InterfaceManager::GetSingleton`'s global) and its refresh for a pad
/// that came or went (`00719630`, thiscall, one word: whether a pad is there).
const INTERFACE_MANAGER: u32 = 0x011d_8a80;
const CONTROLLER_CHANGED: u32 = 0x0071_9630;
/// `[Interface] bDisable360Controller` (an INI pref setting; its value is the byte at +4).
const DISABLE_360_CONTROLLER: u32 = 0x011d_8c4c;
/// XInput's dead zones (XInput.h: `XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE`, `..._RIGHT_THUMB_DEADZONE`,
/// `XINPUT_GAMEPAD_TRIGGER_THRESHOLD`).
const LEFT_DEAD_ZONE: i32 = 7849;
const RIGHT_DEAD_ZONE: i32 = 8689;
const TRIGGER_THRESHOLD: u8 = 30;

pub const FIX: Fix = Fix {
    name: "keyboard and mouse beside the pad",
    env: "NV_PAD_WHEN_USED",
    addresses: &[XINPUT_THUNK],
    summary: "a pad counts as connected only while it is the device in use: the keyboard, the mouse and Esc always work, a run starts with them, and the prompts switch to the pad when it is used",
    install,
};

fn install(e: &mut Engine) {
    // The lock-free state, before the threads start.
    e.ext_free::<State>();
    e.register(XINPUT_THUNK, pad_when_used);
}

/// What the row keeps between reads.
#[derive(Default)]
struct State {
    /// The pad is the device in use (a run starts with the keyboard and mouse).
    pad_in_use: AtomicBool,
    /// The keyboard and mouse use count last seen.
    uses_seen: AtomicU32,
    /// The pad's packet number last seen.
    packet_seen: AtomicU32,
    /// Whether the pad answered at `Idle`'s last read.
    present_at_idle: AtomicBool,
}

/// Whether a pad's state is past XInput's dead zones: a button, a trigger, a stick.
fn active(pad: &PadState) -> bool {
    let past = |axis: i16, zone: i32| i32::from(axis).abs() > zone;
    pad.buttons != 0
        || pad.left_trigger > TRIGGER_THRESHOLD
        || pad.right_trigger > TRIGGER_THRESHOLD
        || past(pad.thumb_lx, LEFT_DEAD_ZONE)
        || past(pad.thumb_ly, LEFT_DEAD_ZONE)
        || past(pad.thumb_rx, RIGHT_DEAD_ZONE)
        || past(pad.thumb_ry, RIGHT_DEAD_ZONE)
}

// Replaces 009f996e (the `XInputGetState` thunk), a row of the fixes preset.
/// `XInputGetState(user, state)` as the thunk passes it on, answering "connected" only while the
/// pad is the device in use (the module's notes).
fn pad_when_used(e: &mut Engine, a: &[u32]) -> Ret {
    let (user, out) = (a[0], a[1]);
    let result = e.call(XINPUT_SLOT, &args![user, out]).u32();
    if user != 0 || out == 0 {
        return result.into_ret();
    }
    let present = result == ERROR_SUCCESS;
    let uses = engine_platform::dinput::keyboard_mouse_uses(e);
    let disabled = e.mem.u8(DISABLE_360_CONTROLLER + 4) != 0;
    let pad = present.then(|| {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&e.mem.bytes(out, 16));
        PadState::from_bytes(&bytes)
    });
    let s = e.ext_free::<State>();
    let keyboard_or_mouse = s.uses_seen.swap(uses, Relaxed) != uses;
    let pad_used =
        pad.is_some_and(|p| s.packet_seen.swap(p.packet, Relaxed) != p.packet && active(&p));
    let in_use = if keyboard_or_mouse || !present || disabled {
        false
    } else {
        pad_used || s.pad_in_use.load(Relaxed)
    };
    s.pad_in_use.store(in_use, Relaxed);
    let present_before = s.present_at_idle.load(Relaxed);
    let answer = if in_use {
        result
    } else {
        ERROR_DEVICE_NOT_CONNECTED
    };
    if out == IDLE_STATE {
        e.ext_free::<State>()
            .present_at_idle
            .store(present, Relaxed);
        let connected = answer == ERROR_SUCCESS;
        let started = e.mem.u32(LAST_HAS_CONTROLLER_GUARD) & 1 != 0;
        let last = e.mem.u8(LAST_HAS_CONTROLLER) != 0;
        if started && last != connected && present == present_before {
            e.mem.set_u8(HAS_360_CONTROLLER, u8::from(connected));
            e.mem.set_u8(LAST_HAS_CONTROLLER, u8::from(connected));
            let manager = e.mem.u32(INTERFACE_MANAGER);
            if manager != 0 {
                e.call(CONTROLLER_CHANGED, &args![manager, u32::from(connected)]);
            }
        }
    }
    answer.into_ret()
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_platform::dinput;
    use engine_platform::xinput::{self, buttons, PadSource};
    use std::sync::{Arc, Mutex};

    /// `Controls::Poll` and `Controls::GetUserAction` (the control reader).
    const CONTROLS_POLL: u32 = 0x00a2_3010;
    const GET_USER_ACTION: u32 = 0x00a2_4660;
    /// `Controls::GetInstance`'s global.
    const CONTROLS_GLOBAL: u32 = 0x011f_35cc;
    /// The game's pad state and its "pad connected" byte.
    const PAD_STATE: u32 = 0x011f_35a8;
    const PAD_CONNECTED: u32 = 0x011f_35c8;
    /// `Controls` +0x2c: the keyboard device.
    const KEYBOARD: u32 = 0x2c;
    /// The fixed control of the menu (Esc, or the pad's Start), asked "went down" (mode 1).
    const MENU: u32 = 0x1c;
    const WENT_DOWN: u32 = 1;
    /// `DIK_ESCAPE`, `DIK_W`.
    const ESCAPE: u8 = 0x01;
    const W: u8 = 0x11;

    /// The system's pads, stood in: user 0's pad or none.
    #[derive(Clone, Default)]
    struct Pads(Arc<Mutex<Option<PadState>>>);

    impl PadSource for Pads {
        fn get_state(&mut self, user: u32, state: &mut [u8; 16]) -> u32 {
            match (user, *self.0.lock().unwrap()) {
                (0, Some(pad)) => {
                    *state = pad.to_bytes();
                    ERROR_SUCCESS
                }
                _ => ERROR_DEVICE_NOT_CONNECTED,
            }
        }
        fn set_state(&mut self, _: u32, _: &[u8; 4]) -> u32 {
            ERROR_DEVICE_NOT_CONNECTED
        }
    }

    /// A pad at rest (`packet`), and with A down.
    fn at_rest(packet: u32) -> PadState {
        PadState {
            packet,
            ..PadState::default()
        }
    }
    fn a_down(packet: u32) -> PadState {
        PadState {
            packet,
            buttons: buttons::A,
            ..PadState::default()
        }
    }

    /// An engine with the translations, the platform's `XInputGetState` at its import slot and the
    /// stand-in pads behind it, the input globals' pages, and a `Controls` object whose keyboard is
    /// the platform's DirectInput keyboard (made, given its format and acquired as `00a22660` does);
    /// with `fix` the row installed as `fixes::install` installs it.
    fn engine(pads: &Pads, fix: bool) -> (Engine, u32) {
        let mut e = Engine::new();
        let (_, _, f) = engine::units::platform::imports()
            .into_iter()
            .find(|(dll, name, _)| dll.eq_ignore_ascii_case("XINPUT1_3.dll") && *name == "#2")
            .expect("the platform's XInputGetState");
        e.register(XINPUT_SLOT, f);
        xinput::set_source(&mut e, Some(Box::new(pads.clone())));
        e.map(0x011d_8000, 0x1000);
        e.map(0x011f_3000, 0x1000);
        let controls = e.mem.alloc(0x2000);
        e.mem.write(controls, &[0; 0x2000]);
        e.mem.set_u32(CONTROLS_GLOBAL, controls);
        let ids = e.mem.alloc(0x40);
        e.mem.write(ids, &dinput::IID_IDIRECTINPUT8A);
        e.mem.write(ids + 0x10, &dinput::GUID_SYS_KEYBOARD);
        let out = ids + 0x20;
        let made = dinput::direct_input8_create(
            &mut e,
            0x0040_0000,
            0x0800,
            Ptr::new(ids),
            Ptr::new(out),
            0,
        );
        assert_eq!(made, 0, "DirectInput8Create");
        let direct_input = e.mem.u32(out);
        assert_eq!(
            e.vcall(direct_input, 0xc, &[ids + 0x10, out, 0]).u32(),
            0,
            "the keyboard"
        );
        let keyboard = e.mem.u32(out);
        let format = e.mem.alloc(0x18);
        e.mem.write(format, &[0; 0x18]);
        e.mem.set_u32(format + 0xc, 0x100);
        assert_eq!(e.vcall(keyboard, 0x2c, &[format]).u32(), 0, "SetDataFormat");
        assert_eq!(e.vcall(keyboard, 0x1c, &[]).u32(), 0, "Acquire");
        e.mem.set_u32(controls + KEYBOARD, keyboard);
        if fix {
            install(&mut e);
        }
        (e, controls)
    }

    /// One frame's poll; then whether the game counts the pad connected and whether its menu control
    /// went down.
    fn frame(e: &mut Engine, controls: u32) -> (bool, bool) {
        e.call(CONTROLS_POLL, &args![controls]);
        let pad = e.mem.u8(PAD_CONNECTED) != 0;
        let menu = e
            .call(GET_USER_ACTION, &args![controls, MENU, WENT_DOWN])
            .u32()
            != 0;
        (pad, menu)
    }

    /// The report (Patrick, 2026-11-21): with a pad that Windows reports, the original counts it
    /// connected from the first frame and its control reader reads only the pad, so Esc is lost.
    #[test]
    fn without_the_row_a_reported_pad_takes_esc_away() {
        let pads = Pads::default();
        *pads.0.lock().unwrap() = Some(at_rest(1));
        let (mut e, controls) = engine(&pads, false);
        assert_eq!(frame(&mut e, controls), (true, false));
        dinput::key(&mut e, ESCAPE, true);
        assert_eq!(frame(&mut e, controls), (true, false), "Esc is not read");
    }

    /// With the row: a run starts with the keyboard (the pad at rest is not in use) and Esc goes
    /// through the game's own reader; a pad press makes the pad the device in use; a key makes the
    /// keyboard it again in the same frame, so the first Esc after using the pad opens the menu;
    /// the mouse moving does the same; the pad's Start opens the menu beside the keyboard.
    #[test]
    fn the_keyboard_and_esc_always_reach_the_game_and_the_pad_counts_when_used() {
        let pads = Pads::default();
        *pads.0.lock().unwrap() = Some(at_rest(1));
        let (mut e, controls) = engine(&pads, true);
        assert_eq!(
            frame(&mut e, controls),
            (false, false),
            "a run starts with the keyboard"
        );
        dinput::key(&mut e, ESCAPE, true);
        assert_eq!(
            frame(&mut e, controls),
            (false, true),
            "Esc reaches the menu control"
        );
        dinput::key(&mut e, ESCAPE, false);
        assert_eq!(frame(&mut e, controls), (false, false));

        // The pad used: it counts; its state is the game's.
        *pads.0.lock().unwrap() = Some(a_down(2));
        assert_eq!(
            frame(&mut e, controls).0,
            true,
            "a pad press makes it the device in use"
        );
        assert_eq!(e.mem.bytes(PAD_STATE, 16), a_down(2).to_bytes().to_vec());
        *pads.0.lock().unwrap() = Some(at_rest(3));
        assert_eq!(frame(&mut e, controls).0, true, "it stays in use at rest");

        // Esc while the pad is in use: read at once.
        dinput::key(&mut e, ESCAPE, true);
        assert_eq!(
            frame(&mut e, controls),
            (false, true),
            "the first Esc opens the menu"
        );
        dinput::key(&mut e, ESCAPE, false);

        // Back to the pad, then the mouse moving takes it back; a key held from before does not
        // count again.
        *pads.0.lock().unwrap() = Some(a_down(4));
        assert_eq!(frame(&mut e, controls).0, true);
        dinput::mouse_move(&mut e, 3, 0, 0);
        assert_eq!(
            frame(&mut e, controls).0,
            false,
            "the mouse moving makes it the device"
        );
        dinput::key(&mut e, W, true);
        *pads.0.lock().unwrap() = Some(a_down(5));
        assert_eq!(
            frame(&mut e, controls).0,
            false,
            "a key in the same frame wins over the pad"
        );
        *pads.0.lock().unwrap() = Some(at_rest(6));
        frame(&mut e, controls);
        *pads.0.lock().unwrap() = Some(a_down(7));
        assert_eq!(
            frame(&mut e, controls).0,
            true,
            "W still held does not take it back"
        );

        // The pad's Start beside the keyboard: the menu control is the keyboard's Esc or the
        // pad's Start (`00a23e10`, its pad button 6).
        dinput::key(&mut e, W, false);
        dinput::mouse_move(&mut e, 1, 0, 0);
        assert_eq!(frame(&mut e, controls).0, false);
        *pads.0.lock().unwrap() = Some(PadState {
            packet: 8,
            buttons: buttons::START,
            ..PadState::default()
        });
        assert_eq!(
            frame(&mut e, controls),
            (true, true),
            "Start opens the menu"
        );

        // Pulled out and plugged in again at rest: the keyboard is in use.
        *pads.0.lock().unwrap() = None;
        assert_eq!(frame(&mut e, controls).0, false);
        *pads.0.lock().unwrap() = Some(at_rest(9));
        assert_eq!(
            frame(&mut e, controls).0,
            false,
            "a pad plugged in counts once used"
        );
    }

    /// `[Interface] bDisable360Controller` set (the setting's value byte; the start menu never
    /// copied it into `Controls` +0, as in the test room): the pad never counts, used or not.
    #[test]
    fn the_disable_setting_holds_without_the_start_menu() {
        let pads = Pads::default();
        *pads.0.lock().unwrap() = Some(at_rest(1));
        let (mut e, controls) = engine(&pads, true);
        e.mem.set_u8(DISABLE_360_CONTROLLER + 4, 1);
        assert_eq!(e.mem.u8(controls), 0, "the input object's switch is clear");
        *pads.0.lock().unwrap() = Some(a_down(2));
        assert_eq!(frame(&mut e, controls), (false, false));
        dinput::key(&mut e, ESCAPE, true);
        assert_eq!(frame(&mut e, controls), (false, true));
    }

    /// `Idle`'s read: a change of device switches `bHas360Controller` and the value `Idle` compares
    /// with together and refreshes the interface with the new answer, so `Idle` has no change to
    /// announce; a pad pulled out while in use is left to `Idle` (its message). The refresh
    /// (`00719630`) is recorded in place of the interface it would refresh.
    #[test]
    fn idles_read_switches_the_prompts_without_the_message() {
        let pads = Pads::default();
        *pads.0.lock().unwrap() = Some(at_rest(1));
        let (mut e, controls) = engine(&pads, true);
        let manager = e.mem.alloc(0x200);
        e.mem.set_u32(INTERFACE_MANAGER, manager);
        static REFRESHES: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());
        REFRESHES.lock().unwrap().clear();
        e.register(CONTROLLER_CHANGED, |_, a| {
            REFRESHES.lock().unwrap().push((a[0], a[1]));
            Ret::default()
        });
        // `Idle` as the game runs it (`idle_controller_state`): the read, `bHas360Controller`, the
        // guard's first value, then the comparison.
        let idle = |e: &mut Engine| -> bool {
            let result = e.call(XINPUT_THUNK, &args![0u32, IDLE_STATE]).u32();
            e.mem.set_u8(HAS_360_CONTROLLER, u8::from(result == 0));
            if e.mem.u32(LAST_HAS_CONTROLLER_GUARD) & 1 == 0 {
                e.mem.set_u32(LAST_HAS_CONTROLLER_GUARD, 1);
                let now = e.mem.u8(HAS_360_CONTROLLER);
                e.mem.set_u8(LAST_HAS_CONTROLLER, now);
            }
            let announce = e.mem.u8(LAST_HAS_CONTROLLER) != e.mem.u8(HAS_360_CONTROLLER);
            let now = e.mem.u8(HAS_360_CONTROLLER);
            e.mem.set_u8(LAST_HAS_CONTROLLER, now);
            announce
        };
        frame(&mut e, controls);
        assert!(!idle(&mut e));
        assert_eq!(
            e.mem.u8(HAS_360_CONTROLLER),
            0,
            "keyboard prompts at the start"
        );

        *pads.0.lock().unwrap() = Some(a_down(2));
        frame(&mut e, controls);
        assert!(!idle(&mut e), "no message for the switch");
        assert_eq!(e.mem.u8(HAS_360_CONTROLLER), 1, "pad prompts");
        assert_eq!(*REFRESHES.lock().unwrap(), vec![(manager, 1)]);

        dinput::key(&mut e, W, true);
        frame(&mut e, controls);
        assert!(!idle(&mut e));
        assert_eq!(e.mem.u8(HAS_360_CONTROLLER), 0, "keyboard prompts again");
        assert_eq!(*REFRESHES.lock().unwrap(), vec![(manager, 1), (manager, 0)]);

        *pads.0.lock().unwrap() = Some(a_down(3));
        frame(&mut e, controls);
        assert!(!idle(&mut e));
        *pads.0.lock().unwrap() = None;
        frame(&mut e, controls);
        assert!(
            idle(&mut e),
            "a pad pulled out while in use: the game's own message"
        );
        assert_eq!(REFRESHES.lock().unwrap().len(), 3);
    }
}
