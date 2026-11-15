//! The phone's Test Range (the Android test build, docs/ANDROID.md; Patrick, 2026-11-15): the
//! game starts straight into the Test Range ([`crate::test_range`]) with one invincible target,
//! obstacles for the active ragdoll's world interaction (a railing, a half-wall, a table's edge, a
//! low obstacle), five guns with ammo kept topped up, and the touch buttons' actions (the next
//! weapon, the target put back). **Not 1:1 with FalloutNV.exe**; only the game's own content, placed
//! the way the console does it. On only when the app turns it on ([`set_on`]) or
//! `NV_TEST_RANGE_PHONE=1`. Nothing is saved: the range refuses every save, and on the phone it does
//! not make its prepared save either.
//!
//! **The obstacles** are statics of the player's own FalloutNV.esm, found by editor ID and size
//! ([`classify`]: a "rail" 55 to 110 units high, a "wall" or "barrier" 55 to 120 high and at least
//! 80 long, a "table" 45 to 90 high, a "curb", "sandbag", "crate", "box", "cinder" or "debris" 8 to
//! 35 high), read on a host thread while the main menu is up ([`start_scan`]); the first of each
//! in the file is used. Placed behind the target as seen from the player (the push of a shot
//! takes it toward them).

use engine::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

static ON: AtomicBool = AtomicBool::new(false);
static SCAN_STARTED: AtomicBool = AtomicBool::new(false);
/// The touch buttons' asks, carried out on the game's thread at its next frame ([`frame`]).
static NEXT_WEAPON_ASKED: AtomicBool = AtomicBool::new(false);
static RESET_ASKED: AtomicBool = AtomicBool::new(false);
/// The weapon in hand (an index of [`GUNS`]) and the frames since the last top-up.
static WEAPON: AtomicU32 = AtomicU32::new(0);
static FRAMES: AtomicU32 = AtomicU32::new(0);

/// The guns (and a melee weapon) the phone's range gives, by editor ID: a pistol, a shotgun, a
/// hunting rifle, an anti-materiel rifle, a sledgehammer.
pub const GUNS: [&str; 5] = [
    "WeapNV9mmPistol",
    "WeapNVHuntingShotgun",
    "WeapHuntingRifle",
    "WeapNVAntiMaterielRifle",
    "WeapSledgehammer",
];
/// How far ahead of the player the target stands (m).
pub const TARGET_METRES: f32 = 6.0;
/// The target's health, kept up (it never dies: essential too).
pub const TARGET_HEALTH: u32 = 100_000;
/// How often (frames) the ammo is topped up and the target healed.
pub const KEEP_EVERY: u32 = 300;

/// An obstacle kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    Railing,
    HalfWall,
    Table,
    Low,
}

impl Piece {
    pub const ALL: [Piece; 4] = [Piece::Railing, Piece::HalfWall, Piece::Table, Piece::Low];

    /// Where it stands from the target (m behind it as the player sees it, m to the player's
    /// right).
    pub fn spot(self) -> (f32, f32) {
        match self {
            Piece::Railing => (1.0, 0.0),
            Piece::HalfWall => (1.0, 2.5),
            Piece::Table => (1.0, -2.5),
            Piece::Low => (0.55, 1.2),
        }
    }
}

/// The obstacles found (kind, form id in FalloutNV.esm), once read.
static PIECES: Mutex<Option<Vec<(Piece, u32)>>> = Mutex::new(None);

/// The phone's range on (the Android app turns it on before the game starts).
pub fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the phone's range is on.
pub fn on() -> bool {
    ON.load(Ordering::Relaxed) || std::env::var("NV_TEST_RANGE_PHONE").is_ok_and(|v| v == "1")
}

/// The touch "weapon" button: the next of [`GUNS`] in hand at the next frame.
pub fn ask_next_weapon() {
    NEXT_WEAPON_ASKED.store(true, Ordering::Relaxed);
}

/// The touch "reset" button: the target put back where it started, at the next frame.
pub fn ask_reset() {
    RESET_ASKED.store(true, Ordering::Relaxed);
}

/// What kind of obstacle a static of editor ID `edid` (lower case) is, by its bounds: its longer
/// and shorter flat sides and its height (game units).
pub fn classify(edid: &str, long: f32, short: f32, height: f32) -> Option<Piece> {
    let has = |w: &[&str]| w.iter().any(|k| edid.contains(k));
    if has(&["rail"]) && (55.0..=110.0).contains(&height) && long >= 60.0 {
        Some(Piece::Railing)
    } else if has(&["wall", "barrier"])
        && (55.0..=120.0).contains(&height)
        && long >= 80.0
        && short <= 40.0
    {
        Some(Piece::HalfWall)
    } else if has(&["table"]) && (45.0..=90.0).contains(&height) && (40.0..=150.0).contains(&long) {
        Some(Piece::Table)
    } else if has(&["curb", "sandbag", "crate", "box", "cinder", "debris"])
        && (8.0..=35.0).contains(&height)
        && long >= 30.0
    {
        Some(Piece::Low)
    } else {
        None
    }
}

/// Reads FalloutNV.esm's statics for the obstacles on a host thread (once, while the main menu is
/// up; only for the phone's range).
pub fn start_scan(e: &engine::Engine) {
    if !on() || SCAN_STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let paths = crate::cell_offsets::loaded_plugin_paths(e);
    let Some(esm) = paths.into_iter().find(|p| {
        p.file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("FalloutNV.esm"))
    }) else {
        eprintln!("Phone range: no FalloutNV.esm among the loaded plugins: no obstacles");
        return;
    };
    let _ = std::thread::Builder::new()
        .name("nv-rs phone range".into())
        .spawn(move || {
            let found = read_pieces(&esm);
            eprintln!(
                "Phone range: obstacles {}",
                found
                    .iter()
                    .map(|(p, id)| format!("{p:?} {id:08X}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            *PIECES.lock().unwrap_or_else(|p| p.into_inner()) = Some(found);
        });
}

fn read_pieces(path: &std::path::Path) -> Vec<(Piece, u32)> {
    let Ok(plugin) = esm::Plugin::open(path) else {
        return Vec::new();
    };
    let mut found: Vec<(Piece, u32)> = Vec::new();
    for entry in plugin.records_of_type(esm::FourCC::new(b"STAT")) {
        let id = entry.header.form_id.0;
        if id >> 24 != 0 {
            continue;
        }
        let Ok(Some(edid)) = plugin.editor_id_of(entry) else {
            continue;
        };
        let Ok(Some(bounds)) = plugin.subrecord_of(entry, esm::FourCC::new(b"OBND")) else {
            continue;
        };
        if bounds.len() < 12 {
            continue;
        }
        let v: Vec<f32> = bounds
            .chunks_exact(2)
            .take(6)
            .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])))
            .collect();
        let (dx, dy, dz) = (v[3] - v[0], v[4] - v[1], v[5] - v[2]);
        if let Some(piece) = classify(&edid.to_ascii_lowercase(), dx.max(dy), dx.min(dy), dz) {
            if !found.iter().any(|(p, _)| *p == piece) {
                found.push((piece, id));
            }
        }
        if found.len() == Piece::ALL.len() {
            break;
        }
    }
    found
}

/// The obstacles found, if read yet.
pub fn pieces() -> Vec<(Piece, u32)> {
    PIECES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .unwrap_or_default()
}

/// The guns' form ids among the loaded weapons (by editor ID, `crate::gun_velocity::editor_id`),
/// in [`GUNS`]' order.
pub fn gun_ids(weapons: &[u32]) -> Vec<u32> {
    GUNS.iter()
        .filter_map(|want| {
            weapons.iter().copied().find(|&id| {
                crate::gun_velocity::editor_id(id).is_some_and(|e| e.eq_ignore_ascii_case(want))
            })
        })
        .collect()
}

/// The buttons' asks and the keeping, once a frame of the range's play: the next gun in hand, the
/// target put back, and every [`KEEP_EVERY`] frames the guns' ammo topped up and the target healed.
pub fn frame(e: &mut Engine, player: u32) {
    if !on() {
        return;
    }
    if NEXT_WEAPON_ASKED.swap(false, Ordering::Relaxed) {
        crate::test_range::phone_next_weapon(e, &WEAPON);
    }
    if RESET_ASKED.swap(false, Ordering::Relaxed) {
        crate::test_range::phone_reset(e, player);
    }
    if FRAMES.fetch_add(1, Ordering::Relaxed) % KEEP_EVERY == KEEP_EVERY - 1 {
        crate::test_range::phone_keep(e);
    }
    crate::room_script::frame(e, player);
}

/// The room's game, shared by the Android app and the Linux room (`android/`, `linux/`): the host
/// started on `data` (the game's Data folder) with the phone's range on, then main's frames one
/// after another on the calling thread, as the viewer runs them (`viewer/src/main.rs`). Never
/// returns. `who` names the app in the log. With a script ([`crate::room_script::start`]) the run
/// ends when it does, or with status 3 when the room is not set up within `NV_ROOM_TIMEOUT`
/// seconds (900).
pub fn run(data: std::path::PathBuf, who: &str) -> ! {
    set_on(true);
    // The game's relative paths (`Data\...`) from its own folder, as the Windows launcher starts it.
    if let Some(folder) = data.parent() {
        if let Err(e) = std::env::set_current_dir(folder) {
            eprintln!("{who}: the game's folder {}: {e}", folder.display());
        }
    }
    // wgpu draws; the fixes preset (the quick start among them); the player's Fallout.ini options
    // read if the game has one.
    if std::env::var_os("NV_D3D9").is_none() {
        std::env::set_var("NV_D3D9", "wgpu");
    }
    let choice = crate::fixes::Choice::read_with(false)
        .or_else(|e| {
            eprintln!("{who}: the settings: {e}; the built-in defaults");
            crate::fixes::Choice::read_with(true)
        })
        .unwrap_or_else(|e| {
            eprintln!("error: {e}");
            crate::end_process(1)
        });
    let mut game = match crate::Host::start_with(&data, &choice) {
        Ok(host) => host,
        Err(e) => {
            eprintln!("error: {e}");
            crate::end_process(1);
        }
    };
    println!("{}", game.describe());
    game.at_main_end = Some(Box::new(
        |_game: &crate::Host, end: crate::player_move::MainEnd| {
            crate::run_exit_hooks();
            crate::end_process(end.status());
        },
    ));
    game.end_if_main_ended();
    let timeout = std::env::var("NV_ROOM_TIMEOUT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(900);
    let began = std::time::Instant::now();
    let mut last = began;
    loop {
        game.set_keys_down(&[]);
        game.mouse([0; 3], crate::room_script::buttons());
        if !game.main_frame() {
            break;
        }
        if last.elapsed() >= std::time::Duration::from_secs(5) {
            last = std::time::Instant::now();
            println!("{}", game.movement.summary());
            if crate::room_script::on()
                && !crate::room_script::started()
                && began.elapsed().as_secs() >= timeout
            {
                eprintln!(
                    "error: {who}: the room was not set up within {timeout} s (NV_ROOM_TIMEOUT); \
                     the range's phase: {:?}",
                    crate::test_range::phase()
                );
                crate::run_exit_hooks();
                crate::end_process(3);
            }
        }
    }
    eprintln!(
        "error: the game's main is not running ({})",
        game.movement.summary()
    );
    crate::end_process(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obstacles_are_told_by_name_and_size() {
        assert_eq!(
            classify("woodrailing01", 128.0, 8.0, 70.0),
            Some(Piece::Railing)
        );
        assert_eq!(
            classify("concretebarrier01", 120.0, 30.0, 80.0),
            Some(Piece::HalfWall)
        );
        assert_eq!(
            classify("diningtable01", 100.0, 60.0, 55.0),
            Some(Piece::Table)
        );
        assert_eq!(
            classify("curbstraight01", 128.0, 20.0, 15.0),
            Some(Piece::Low)
        );
        assert_eq!(
            classify("woodrailing01", 128.0, 8.0, 200.0),
            None,
            "too tall"
        );
        assert_eq!(classify("barrel01", 40.0, 40.0, 50.0), None);
    }
}
