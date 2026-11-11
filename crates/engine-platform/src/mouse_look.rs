//! How the real game window's mouse reaches the game (`NV_MOUSE_LOOK`): what `d3d9real`'s window
//! procedure decides on each mouse message, kept here so it is the same and tested on every
//! platform.
//!
//! The original (FalloutNV.exe 1.4.0.525) makes its DirectInput mouse exclusive and foreground:
//! `Controls`' mouse device gets `SetCooperativeLevel(window, DISCL_EXCLUSIVE | DISCL_FOREGROUND)`
//! (`crates/engine-fallout-unplaced-b`, `run_00a237f0`, slot `0x34`) and `Controls::Poll`
//! (`00a23010`) acquires it each frame. With such a device Windows hides the cursor and keeps it in
//! the window whenever the game's window is in front, menus or not (the game draws its own cursor
//! in its menus, moved by the same DirectInput counts), and lets it go when another window comes to
//! the front (Alt+Tab). Every mouse movement while the game has the mouse reaches it: the player's
//! look (`PlayerCharacter`'s `009445b0`, mode 0, through `Controls`' axes `00a239e0`) turns the view
//! with no button held, and the right button is the game's own control (aim, block). That is
//! [`MouseLook::Held`], the default.
//!
//! [`MouseLook::RightButton`] (`NV_MOUSE_LOOK=hold`) is not the original's: a debugging aid that
//! leaves the cursor free and visible (for a debugger or another window beside the game). The mouse
//! turns the view only while the right button is held, and that button does not reach the game.

/// How the mouse reaches the game (`NV_MOUSE_LOOK`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseLook {
    /// The original's: while the window is in front, the cursor is hidden and held in it, every
    /// movement reaches the game, and every button is the game's (unset, or `captured`).
    Held,
    /// A debugging aid (`hold`): the cursor stays free; the view turns only while the right
    /// button is held, and the right button does not reach the game.
    RightButton,
}

/// DirectInput's index of the right mouse button (`DIMOUSESTATE2::rgbButtons[1]`).
pub const RIGHT_BUTTON: usize = 1;

impl MouseLook {
    /// The way a value of `NV_MOUSE_LOOK` names (`None`: unset): `captured` (or unset or empty)
    /// for the original's, `hold` for the debugging aid, case and spaces aside. Any other value is
    /// an error that names the two (the caller keeps the original's).
    pub fn parse(value: Option<&str>) -> Result<MouseLook, String> {
        let Some(value) = value else {
            return Ok(MouseLook::Held);
        };
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "captured" => Ok(MouseLook::Held),
            "hold" => Ok(MouseLook::RightButton),
            _ => Err(format!(
                "NV_MOUSE_LOOK={}: use captured (the original's, the default) or hold \
                 (the view turns only while the right button is held); the original's is kept",
                value.trim()
            )),
        }
    }

    /// The run's choice (read once): `NV_MOUSE_LOOK`; a value it can't read is printed once and
    /// the original's is kept.
    pub fn asked() -> MouseLook {
        static ASKED: std::sync::OnceLock<MouseLook> = std::sync::OnceLock::new();
        *ASKED.get_or_init(|| {
            match MouseLook::parse(std::env::var("NV_MOUSE_LOOK").ok().as_deref()) {
                Ok(look) => look,
                Err(message) => {
                    eprintln!("{message}");
                    MouseLook::Held
                }
            }
        })
    }

    /// Whether a click of `button` (DirectInput's index) in the window's client area takes the
    /// cursor. Held: any click (Windows gives an exclusive foreground mouse to the window clicked
    /// into). Right button: only the right one, which holds it until it comes up
    /// ([`MouseLook::lets_go_on_release`]).
    pub fn takes_on_click(self, button: usize) -> bool {
        match self {
            MouseLook::Held => true,
            MouseLook::RightButton => button == RIGHT_BUTTON,
        }
    }

    /// Whether `button` coming up lets go of the cursor (only the debugging aid's right button).
    pub fn lets_go_on_release(self, button: usize) -> bool {
        self == MouseLook::RightButton && button == RIGHT_BUTTON
    }

    /// Whether the window takes the cursor when it is activated from the keyboard (Alt+Tab,
    /// `WA_ACTIVE`) or a move or size of it ends with the window in front (`WM_EXITSIZEMOVE`): the
    /// original's game acquires its mouse again on its next frame.
    pub fn takes_on_activation(self) -> bool {
        self == MouseLook::Held
    }

    /// Whether a raw mouse movement takes the cursor first: with the original's, when the window
    /// is in front (`foreground`), is not being moved or sized, the cursor is not held yet and no
    /// mouse button is down (a button down there was pressed outside the client area, on the title
    /// bar or a border: the cursor stays free until it is released). This covers each way the
    /// window comes to the front without a click in it or `WA_ACTIVE`: a click on its title bar
    /// (`WA_CLICKACTIVE`), or a start the window could not take the focus at once.
    pub fn takes_on_motion(
        self,
        held: bool,
        moving: bool,
        buttons_down: bool,
        foreground: bool,
    ) -> bool {
        self == MouseLook::Held && !held && !moving && !buttons_down && foreground
    }

    /// Whether a raw mouse movement reaches the game. Held: unless the window is being moved or
    /// sized (the original's game, whose window procedure runs on its main thread, reads nothing
    /// while Windows moves its window). Right button: only while the right button holds the
    /// cursor.
    pub fn motion_reaches_game(self, held: bool, moving: bool) -> bool {
        match self {
            MouseLook::Held => !moving,
            MouseLook::RightButton => held,
        }
    }

    /// Whether `button` (DirectInput's index) reaches the game's mouse: every one with the
    /// original's (the right one is the game's aim and block); with the debugging aid the right
    /// one turns the view instead.
    pub fn button_reaches_game(self, button: usize) -> bool {
        match self {
            MouseLook::Held => true,
            MouseLook::RightButton => button != RIGHT_BUTTON,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_empty_and_captured_are_the_originals() {
        assert_eq!(MouseLook::parse(None), Ok(MouseLook::Held));
        assert_eq!(MouseLook::parse(Some("")), Ok(MouseLook::Held));
        assert_eq!(MouseLook::parse(Some(" Captured ")), Ok(MouseLook::Held));
        assert_eq!(MouseLook::parse(Some("HOLD")), Ok(MouseLook::RightButton));
        let wrong = MouseLook::parse(Some("toggle")).unwrap_err();
        assert!(wrong.contains("NV_MOUSE_LOOK=toggle") && wrong.contains("hold"));
    }

    #[test]
    fn the_originals_mouse_turns_the_view_with_no_button_and_every_button_reaches_the_game() {
        let look = MouseLook::Held;
        // Every movement, held or not yet held, reaches the game; none while the window moves.
        assert!(look.motion_reaches_game(true, false));
        assert!(look.motion_reaches_game(false, false));
        assert!(!look.motion_reaches_game(false, true));
        // The right button is the game's (aim, block), as are the others.
        for button in 0..8 {
            assert!(look.button_reaches_game(button));
            assert!(look.takes_on_click(button));
            assert!(!look.lets_go_on_release(button));
        }
        assert!(look.takes_on_activation());
    }

    #[test]
    fn the_original_takes_the_cursor_on_motion_only_in_front_and_with_no_button_down() {
        let look = MouseLook::Held;
        assert!(look.takes_on_motion(false, false, false, true));
        // Already held, being moved, a button down (title bar), or another window in front.
        assert!(!look.takes_on_motion(true, false, false, true));
        assert!(!look.takes_on_motion(false, true, false, true));
        assert!(!look.takes_on_motion(false, false, true, true));
        assert!(!look.takes_on_motion(false, false, false, false));
    }

    #[test]
    fn the_debugging_aid_turns_the_view_only_while_the_right_button_holds_the_cursor() {
        let look = MouseLook::RightButton;
        assert!(look.motion_reaches_game(true, false));
        assert!(!look.motion_reaches_game(false, false));
        assert!(!look.button_reaches_game(RIGHT_BUTTON));
        assert!(look.button_reaches_game(0));
        assert!(look.takes_on_click(RIGHT_BUTTON) && !look.takes_on_click(0));
        assert!(look.lets_go_on_release(RIGHT_BUTTON) && !look.lets_go_on_release(0));
        assert!(!look.takes_on_activation());
        assert!(!look.takes_on_motion(false, false, false, true));
    }
}
