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

// The raw mouse's movement, as `d3d9real`'s `raw_mouse` turns it into the counts the game reads.
//
// Two things there are not movement of the mouse, and one movement is not in counts:
//
// - The recentring. While the window holds the cursor, the window puts it back at the client
//   area's centre when it strays (so it never rests on the area's edge, where it showed: playtest
//   2026-11-03). On some PCs Windows reports that `SetCursorPos` as a raw relative movement of the
//   warp's own size and direction: added to the counts, it took back the player's movement, and the
//   game's menu cursor stayed in the middle of the screen (playtest, 2026-11-12). Such an event
//   comes from no window message the warp sends (raw input is posted, and `SetCursorPos` doesn't
//   take messages), so it is recognised by its size and direction instead ([`RawMotion::relative`]).
//   The cursor is put back only once it is a quarter of the client area from the centre
//   ([`should_recentre`]), so a warp is far larger than any one movement the mouse reports and a
//   real movement is never mistaken for one; a PC without the echo loses nothing.
// - The absolute position (`MOUSE_MOVE_ABSOLUTE`, from tablets, some touchpads, remote and virtual
//   machines' mice, and on some PCs the warp itself): 0 to 65535 across the primary monitor or,
//   with `MOUSE_VIRTUAL_DESKTOP`, all of them. It was left out, so such a mouse never moved the game
//   at all. It becomes the movement since that device's last position ([`RawMotion::absolute`]);
//   a warp sets the position of the device-less events Windows makes itself (handle 0) to its
//   target, so its absolute echo moves nothing.

/// The steps of an absolute position across the screen: 0 to 65535 (`MOUSE_MOVE_ABSOLUTE`).
const ABSOLUTE_STEPS: i64 = 65_536;

/// How many raw events after a warp its echo is waited for: it comes right after the events
/// already queued before the warp.
const ECHO_EVENTS: u8 = 4;

/// The pixel `value` (an absolute raw position, 0 to 65535 on each axis) names on `screen`
/// (left, top, width, height: the primary monitor, or the virtual desktop with
/// `MOUSE_VIRTUAL_DESKTOP`).
pub fn absolute_to_pixels(value: [i32; 2], screen: [i32; 4]) -> [i32; 2] {
    let along = |v: i32, start: i32, size: i32| {
        let v = i64::from(v.clamp(0, 65_535));
        start + (v * i64::from(size.max(1)) / ABSOLUTE_STEPS) as i32
    };
    [
        along(value[0], screen[0], screen[2]),
        along(value[1], screen[1], screen[3]),
    ]
}

/// Whether the held cursor, `offset` pixels from the client area's centre, is put back there:
/// once it is more than half way to the area's edge on either axis (`half`: half the area's width
/// and height), well before the edge.
pub fn should_recentre(offset: [i32; 2], half: [i32; 2]) -> bool {
    offset
        .iter()
        .zip(half)
        .any(|(&o, h)| o.unsigned_abs() > (h.max(2) / 2) as u32)
}

/// Whether a raw relative movement `delta` is the echo of a warp by `warp` pixels: the same
/// direction (within about 11 degrees) and 0.75 to 2.5 times its length (with display scaling the
/// warp is given in scaled pixels and may be reported in the screen's).
pub fn is_echo(warp: [i32; 2], delta: [i32; 2]) -> bool {
    let (wx, wy) = (i64::from(warp[0]), i64::from(warp[1]));
    let (dx, dy) = (i64::from(delta[0]), i64::from(delta[1]));
    let length = wx * wx + wy * wy;
    if length == 0 {
        return false;
    }
    let dot = dx * wx + dy * wy;
    let cross = dx * wy - dy * wx;
    4 * dot >= 3 * length && 2 * dot <= 5 * length && 25 * cross * cross <= dot * dot
}

/// What turns the raw mouse's events into movement: each absolute device's last position, and
/// the warps whose echo may still come.
#[derive(Debug, Default)]
pub struct RawMotion {
    /// The last position (pixels) of each device that reported absolute positions, by its raw
    /// input handle (0: the events Windows makes itself).
    positions: Vec<(isize, [i32; 2])>,
    /// Warps (pixels) whose echo may still come, with the raw events left to wait for it.
    warps: Vec<([i32; 2], u8)>,
}

impl RawMotion {
    /// A relative movement: `None` when it is the echo of a warp (the warp is then no longer
    /// waited for), else the movement.
    pub fn relative(&mut self, delta: [i32; 2]) -> Option<[i32; 2]> {
        let echo = self.warps.iter().position(|&(w, _)| is_echo(w, delta));
        if let Some(at) = echo {
            self.warps.remove(at);
            return None;
        }
        self.age();
        Some(delta)
    }

    /// An absolute position `at` (pixels) of the device `device`: the movement since its last
    /// one (none for its first).
    pub fn absolute(&mut self, device: isize, at: [i32; 2]) -> [i32; 2] {
        self.age();
        match self.positions.iter_mut().find(|(d, _)| *d == device) {
            Some((_, last)) => {
                let delta = [at[0] - last[0], at[1] - last[1]];
                *last = at;
                delta
            }
            None => {
                self.positions.push((device, at));
                [0, 0]
            }
        }
    }

    /// The window put the cursor at `target` (pixels), `warp` from where it was: its echo is
    /// waited for, and an absolute one moves nothing.
    pub fn warped(&mut self, warp: [i32; 2], target: [i32; 2]) {
        if warp != [0, 0] {
            self.warps.push((warp, ECHO_EVENTS));
        }
        match self.positions.iter_mut().find(|(d, _)| *d == 0) {
            Some((_, last)) => *last = target,
            None => self.positions.push((0, target)),
        }
    }

    /// The window let go of the cursor: the positions and warps are forgotten (the next absolute
    /// event of each device starts again without a movement).
    pub fn reset(&mut self) {
        self.positions.clear();
        self.warps.clear();
    }

    /// One more raw event that was not a warp's echo.
    fn age(&mut self) {
        for (_, left) in &mut self.warps {
            *left -= 1;
        }
        self.warps.retain(|&(_, left)| left > 0);
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

    #[test]
    fn absolute_positions_cover_the_screen_they_name() {
        let primary = [0, 0, 2560, 1440];
        assert_eq!(absolute_to_pixels([0, 0], primary), [0, 0]);
        assert_eq!(absolute_to_pixels([32_768, 32_768], primary), [1280, 720]);
        assert_eq!(absolute_to_pixels([65_535, 65_535], primary), [2559, 1439]);
        // The virtual desktop with a monitor left of the primary one.
        let desktop = [-1920, 0, 4480, 1440];
        assert_eq!(absolute_to_pixels([0, 0], desktop), [-1920, 0]);
        assert_eq!(absolute_to_pixels([65_535, 0], desktop), [2559, 0]);
    }

    #[test]
    fn an_absolute_device_moves_by_the_difference_of_its_positions() {
        let mut motion = RawMotion::default();
        // Its first position is only where it is; then each one moves by the difference.
        assert_eq!(motion.absolute(7, [100, 100]), [0, 0]);
        assert_eq!(motion.absolute(7, [103, 98]), [3, -2]);
        // Another device keeps its own position.
        assert_eq!(motion.absolute(9, [500, 500]), [0, 0]);
        assert_eq!(motion.absolute(7, [104, 98]), [1, 0]);
        // Let go of: each starts again.
        motion.reset();
        assert_eq!(motion.absolute(7, [900, 900]), [0, 0]);
    }

    #[test]
    fn a_warps_absolute_echo_moves_nothing() {
        let mut motion = RawMotion::default();
        assert_eq!(motion.absolute(0, [1500, 800]), [0, 0]);
        motion.warped([-220, -80], [1280, 720]);
        assert_eq!(motion.absolute(0, [1280, 720]), [0, 0]);
        // A real absolute device is not moved by the warp: its next position is its own.
        assert_eq!(motion.absolute(5, [10, 10]), [0, 0]);
        motion.warped([-5, 0], [1280, 720]);
        assert_eq!(motion.absolute(5, [12, 10]), [2, 0]);
    }

    #[test]
    fn a_warps_relative_echo_is_left_out_once() {
        let mut motion = RawMotion::default();
        motion.warped([-700, 30], [1280, 720]);
        // The player's movement before the echo (queued before the warp) still counts.
        assert_eq!(motion.relative([12, -1]), Some([12, -1]));
        assert_eq!(motion.relative([-700, 30]), None);
        // Only once: the same movement again is movement.
        assert_eq!(motion.relative([-700, 30]), Some([-700, 30]));
        // With display scaling (150%) the echo is in the screen's pixels.
        motion.warped([-400, 0], [1280, 720]);
        assert_eq!(motion.relative([-600, 1]), None);
    }

    #[test]
    fn without_an_echo_no_movement_is_lost() {
        // A PC that reports no echo: every movement after a warp counts, whatever its direction,
        // and the warp stops being waited for after a few events.
        let mut motion = RawMotion::default();
        motion.warped([-700, 0], [1280, 720]);
        for delta in [[3, 0], [-3, 0], [-40, 2], [0, 5], [-25, 0]] {
            assert_eq!(motion.relative(delta), Some(delta));
        }
        assert_eq!(motion.relative([-700, 0]), Some([-700, 0]));
    }

    #[test]
    fn an_echo_has_the_warps_direction_and_about_its_length() {
        assert!(is_echo([-640, 0], [-640, 0]));
        assert!(is_echo([-640, 0], [-1280, 0]), "200% scaling");
        assert!(is_echo([300, -400], [300, -395]));
        assert!(!is_echo([-640, 0], [640, 0]), "the other way");
        assert!(!is_echo([-640, 0], [-200, 0]), "too short");
        assert!(!is_echo([-640, 0], [-2000, 0]), "too long");
        assert!(!is_echo([-640, 0], [-640, 400]), "another direction");
        assert!(!is_echo([0, 0], [0, 0]));
    }

    #[test]
    fn the_cursor_is_put_back_half_way_to_the_edge() {
        let half = [1280, 720];
        assert!(!should_recentre([0, 0], half));
        assert!(!should_recentre([640, -360], half));
        assert!(should_recentre([641, 0], half));
        assert!(should_recentre([0, -361], half));
    }
}
