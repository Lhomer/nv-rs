//! Weighty combat (`NV_WEIGHTY_COMBAT`), a row of the `fixes` preset ([`crate::fixes`],
//! docs/FIXES.md, plan in docs/GUNPLAY.md): **not 1:1 with FalloutNV.exe**, an nv-rs addition the
//! player asked for (Patrick, 2026-11-13: guns and melee that feel weighty, with recoil, kick,
//! inertia, impact and reach, and hits that push bodies by calibre or weapon weight and can
//! sever limbs; 2026-11-13 again: the sway must show, and no slow motion on hits). In the `vanilla` preset, or with `bWeightyCombat=0`, nothing here runs.
//!
//! Everything is procedural (code-driven): no animation, model or sound is added or shipped, and
//! nothing is taken from any other game. The game's own systems do the rest: its look functions
//! turn the view, its time multiplier slows time, its kill impulse pushes the body, and its own
//! dismember and explode outcome severs the limb.
//!
//! **The hooks** (each wraps the translation, which runs as it is):
//! - `TESObjectWEAP::Fire` (`00523150`): a shot of the player's gun (types 3 to 9) adds a view
//!   kick ([`kick_degrees`]: from the weapon's damage and projectiles, less for automatic and
//!   held-trigger weapons, less in iron sights) and a push of the weapon back towards the player.
//! - The player's look handling (`009445b0`, once a frame in play): the kick is turned in over a
//!   few hundredths of a second with the game's own clamped look functions (`00931e50` pitch,
//!   `00931d30` heading), a share of it comes back afterwards (less what the player already pulled
//!   down), the impact punches' camera part is turned in and back, and the viewmodel's pose for
//!   the frame is worked out: the sway's springs ([`Sway`]) and the punches' viewmodel part.
//! - `Main::DrawWorld_MTSetup1stPerson` (`00874c10`, each frame just before the first-person
//!   draw): the pose goes into the first-person root as the game updates it ([`place_pose`]).
//!   The root's own local translation is zeroed and the root updated in there, so an offset
//!   written anywhere earlier in the frame never shows; this is where the arms and the weapon get
//!   their place for the draw.
//! - The reach search (`009a60e0`, the actor and its reach): the player's melee reach is scaled.
//! - `Actor::CombatHit` (`00899cb0`): the player's melee hit that connects punches the viewmodel
//!   and the camera for 60 to 120 ms, harder for a heavier weapon ([`melee_punch`]); a heavy shot
//!   does the same in `Fire` ([`shot_punch`]). Game time is never changed: nothing here writes the
//!   time multiplier.
//! - The kill impulse (`008ae000`, the aggressor and the hit data): the strength the game pushes
//!   the killed body with (the hit's health damage at `+0x14`, times the weapon's kill impulse,
//!   at the hit's point) is scaled, by the weapon's weight for melee.
//! - The dismember roll (`00646e50`, called by `HitData::DetermineOutcome` for a killing hit on a
//!   limb): a weapon whose force ([`force`]) reaches `iDismemberForce` severs the limb, and a gun
//!   whose force reaches `iExplodeForce` explodes an explodable one, as if the roll had said so;
//!   the weapon's own override (`008a16b0`) still applies, and the game's body part data decides
//!   later whether the limb can be severed at all.
//!
//! **The tuning** ([`Tuning`]): Fallout.ini `[nv-rs Gunplay]`, in a launcher's play run, and the
//! `NV_GUNPLAY_*` variables, which win. Five rows of the game's Settings menu (Display page, after
//! the graphics rows) change the main ones live and save them ([`ROWS`]).

use crate::fixes::Fix;
use engine::prelude::*;
use engine::units::fallout_ai::actor_p3::actor_combat_hit;
use engine::units::fallout_ai::actor_p4::fn_008ae000;
use engine::units::fallout_ai::playercharacter::fn_009445b0;
use engine::units::fallout_ai::run_009a6050::fn_009a60e0;
use engine::units::fallout_interface::startmenu::start_menu_create_menu_elements;
use engine::units::fallout_misc::main_p2::main_draw_world_mt_setup_1st_person;
use engine::units::fallout_shared::combatformulas::fn_00646e50;
use engine::units::fallout_shared::tesobjectweap::fn_00523150;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// `TESObjectWEAP::Fire` (`thiscall`, the shooter).
pub const WEAPON_FIRE: u32 = 0x0052_3150;
/// The player's look handling (`thiscall`: frame time, aim pressed, flags, unused).
pub const LOOK_INPUT: u32 = 0x0094_45b0;
/// The reach search (`cdecl`: the actor, its reach); returns what it can hit.
pub const REACH_SEARCH: u32 = 0x009a_60e0;
/// `Actor::CombatHit` (`thiscall`: target, flag, source, kind).
pub const COMBAT_HIT: u32 = 0x0089_9cb0;
/// The kill impulse (`thiscall` on the aggressor: the hit data).
pub const KILL_IMPULSE: u32 = 0x008a_e000;
/// `Main::DrawWorld_MTSetup1stPerson` (`thiscall` on `Main`): the first-person root's update and
/// camera, just before the first-person draw.
pub const FIRST_PERSON_SETUP: u32 = 0x0087_4c10;
/// The dismember and explode roll (`cdecl`, eight words).
pub const DISMEMBER_ROLL: u32 = 0x0064_6e50;
/// `StartMenu::CreateMenuElements`, shared with [`crate::fov_slider`].
pub const CREATE_MENU_ELEMENTS: u32 = crate::fov_slider::CREATE_MENU_ELEMENTS;

/// The row in [`crate::fixes::FIXES`].
pub const FIX: Fix = Fix {
    name: "weighty combat",
    env: "NV_WEIGHTY_COMBAT",
    addresses: &[
        WEAPON_FIRE,
        LOOK_INPUT,
        FIRST_PERSON_SETUP,
        REACH_SEARCH,
        COMBAT_HIT,
        KILL_IMPULSE,
        DISMEMBER_ROLL,
        CREATE_MENU_ELEMENTS,
    ],
    summary: "guns kick the view and recover, the weapon lags fast turns and settles back, melee hits and heavy shots punch the viewmodel and camera (game time untouched), melee reaches a little further, kills push bodies by calibre and weapon weight, and a heavy enough hit severs the limb (Settings menu rows and [nv-rs Gunplay] tune it)",
    install,
};

/// The Fallout.ini section of the tuning.
pub const SECTION: &str = "nv-rs Gunplay";

/// The strengths and switches (module documentation). Percentages are of the built-in feel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tuning {
    /// `iRecoil`: the view kick, 0 for none.
    pub recoil: i32,
    /// `iRecoilRecovery`: the share of the kick that comes back.
    pub recovery: i32,
    /// `iSway`: the weapon's sway and inertia, 0 for none.
    pub sway: i32,
    /// `bHitStop`: a melee hit that connects and a heavy shot punch the viewmodel and the camera
    /// (the impact punch; the key keeps its first name). Game time is never slowed.
    pub hit_stop: bool,
    /// `iHitStopMs`: how long the impact punch lasts, in milliseconds, for a light weapon (heavier
    /// ones up to 1.3 times); 0 = no punch.
    pub hit_stop_ms: i32,
    /// `iMeleeReach`: the player's melee reach, 100 for the game's.
    pub melee_reach: i32,
    /// `iImpulse`: the push a kill gives the body, 100 for the game's.
    pub impulse: i32,
    /// `iDismemberForce`: the force ([`force`]) from which a killing hit severs the limb; 0 = only
    /// the game's own chance.
    pub dismember_force: i32,
    /// `iExplodeForce`: the force from which a gun's killing hit explodes an explodable limb; 0 =
    /// only the game's own chance.
    pub explode_force: i32,
}

impl Tuning {
    /// The built-in feel: the values of `crates/host/data/game_ini.tsv` (a test holds them).
    pub const PLAY: Tuning = Tuning {
        recoil: 100,
        recovery: 70,
        sway: 100,
        hit_stop: true,
        hit_stop_ms: 90,
        melee_reach: 115,
        impulse: 150,
        dismember_force: 45,
        explode_force: 110,
    };

    /// Sets `key` (a [`KEYS`] key) to `value`; false for a key that isn't one or a value out of
    /// its range.
    pub fn set(&mut self, key: &str, value: i32) -> bool {
        let Some(&(_, _, low, high)) = KEYS.iter().find(|k| k.0.eq_ignore_ascii_case(key)) else {
            return false;
        };
        if !(low..=high).contains(&value) {
            return false;
        }
        match key.to_ascii_lowercase().as_str() {
            "irecoil" => self.recoil = value,
            "irecoilrecovery" => self.recovery = value,
            "isway" => self.sway = value,
            "bhitstop" => self.hit_stop = value != 0,
            "ihitstopms" => self.hit_stop_ms = value,
            "imeleereach" => self.melee_reach = value,
            "iimpulse" => self.impulse = value,
            "idismemberforce" => self.dismember_force = value,
            "iexplodeforce" => self.explode_force = value,
            _ => return false,
        }
        true
    }

    /// The value of `key` (a [`KEYS`] key).
    pub fn get(&self, key: &str) -> Option<i32> {
        Some(match key.to_ascii_lowercase().as_str() {
            "irecoil" => self.recoil,
            "irecoilrecovery" => self.recovery,
            "isway" => self.sway,
            "bhitstop" => i32::from(self.hit_stop),
            "ihitstopms" => self.hit_stop_ms,
            "imeleereach" => self.melee_reach,
            "iimpulse" => self.impulse,
            "idismemberforce" => self.dismember_force,
            "iexplodeforce" => self.explode_force,
            _ => return None,
        })
    }

    /// The tuning from Fallout.ini's values (`ini`, as [`parse`] read them) and the environment,
    /// which wins: a variable that is set and not a usable number stops the start.
    pub fn from_parts(
        ini: &[(&'static str, i32)],
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Tuning, String> {
        let mut tuning = Tuning::PLAY;
        for &(key, value) in ini {
            tuning.set(key, value);
        }
        for &(key, variable, low, high) in &KEYS {
            let Some(raw) = env(variable).filter(|v| !v.trim().is_empty()) else {
                continue;
            };
            match parse(key, raw.trim()) {
                Some(value) if tuning.set(key, value) => {}
                _ => {
                    return Err(format!(
                        "{variable}={raw}: use a whole number from {low} to {high}"
                    ))
                }
            }
        }
        Ok(tuning)
    }
}

impl Default for Tuning {
    fn default() -> Tuning {
        Tuning::PLAY
    }
}

/// The keys of [`SECTION`]: key, its environment variable, lowest and highest value.
pub const KEYS: [(&str, &str, i32, i32); 9] = [
    ("iRecoil", "NV_GUNPLAY_RECOIL", 0, 300),
    ("iRecoilRecovery", "NV_GUNPLAY_RECOVERY", 0, 100),
    ("iSway", "NV_GUNPLAY_SWAY", 0, 300),
    ("bHitStop", "NV_GUNPLAY_HIT_STOP", 0, 1),
    ("iHitStopMs", "NV_GUNPLAY_HIT_STOP_MS", 0, 250),
    ("iMeleeReach", "NV_GUNPLAY_MELEE_REACH", 50, 200),
    ("iImpulse", "NV_GUNPLAY_IMPULSE", 0, 500),
    ("iDismemberForce", "NV_GUNPLAY_DISMEMBER_FORCE", 0, 1000),
    ("iExplodeForce", "NV_GUNPLAY_EXPLODE_FORCE", 0, 1000),
];

/// A value of `key` as the file or a variable holds it: `1`/`0` for a `b` key, a whole number for
/// an `i` key; `None` when it isn't one or is out of the key's range.
pub fn parse(key: &str, raw: &str) -> Option<i32> {
    let &(_, _, low, high) = KEYS.iter().find(|k| k.0.eq_ignore_ascii_case(key))?;
    let value = if key.starts_with(['b', 'B']) {
        match raw {
            "1" => 1,
            "0" => 0,
            _ => return None,
        }
    } else {
        raw.parse().ok()?
    };
    (low..=high).contains(&value).then_some(value)
}

/// The tuning in force, and whether the row is on now (the Settings menu's Weighty Combat row
/// turns it off and on again in play; the hooks stay, and pass through while it is off).
static TUNING: Mutex<Tuning> = Mutex::new(Tuning::PLAY);
static ON: AtomicBool = AtomicBool::new(true);

/// The tuning the run starts with (`crate::fixes::install`, before the hooks).
pub fn set_tuning(tuning: Tuning) {
    *TUNING.lock().unwrap_or_else(|p| p.into_inner()) = tuning;
}

/// The tuning in force.
pub fn tuning() -> Tuning {
    *TUNING.lock().unwrap_or_else(|p| p.into_inner())
}

fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

// --- The game's words. ---------------------------------------------------------------------

/// The player (`PlayerCharacter *`).
const PLAYER: u32 = 0x011d_ea3c;
/// The VATS mode (0 none, 4 the kill cam), as [`crate::fov_slider`] reads it.
const VATS_MODE: u32 = 0x011f_2250 + 8;
/// `PlayerCharacter::b3rdPerson`.
const THIRD_PERSON: u32 = 0x64a;
/// The player's node (`00950bb0`, `thiscall`: 1 the first-person root); the camera node the
/// first-person camera is placed from (`011e07d0`); an `NiAVObject`'s parent (`+0x18`), local
/// rotation (`m_kLocal`, `+0x34`, nine floats), world rotation (`m_kWorld`, `+0x68`) and world
/// translation (`+0x8c`).
const GET_PLAYER_NODE: u32 = 0x0095_0bb0;
const CAMERA_NODE: u32 = 0x011e_07d0;
const PARENT: u32 = 0x18;
const LOCAL_ROTATION: u32 = 0x34;
const WORLD_ROTATION: u32 = 0x68;
const WORLD_TRANSLATION: u32 = 0x8c;
/// A reference's position (`TESObjectREFR::data.position`).
const POSITION: u32 = 0x30;
/// `PlayerCharacter::IsPipBoyActive`, `Actor::GetIronSights` (both `thiscall`, a byte).
const PLAYER_IS_PIPBOY_ACTIVE: u32 = 0x0096_7ae0;
const ACTOR_GET_IRON_SIGHTS: u32 = 0x008b_bc10;
/// `MobileObject`: the looking angle (pitch, `00931d70`, ST0), adding to it with the clamp
/// (`00931e50`) and adding to the heading (`00931d30`); the heading getter is virtual `+0x2bc`.
const GET_LOOKING: u32 = 0x0093_1d70;
const ADD_LOOKING: u32 = 0x0093_1e50;
const ADD_HEADING: u32 = 0x0093_1d30;
const HEADING_SLOT: u32 = 0x2bc;
/// The weapon's override of the roll's outcome (`008a16b0`: 1 sever only, 2 explode only, 3
/// neither).
const ROLL_OVERRIDE: u32 = 0x008a_16b0;
/// An actor's process (`+0x68`) and its held item (virtual `+0x148`); an item's form (`+8`).
const ACTOR_PROCESS: u32 = 0x68;
const PROCESS_HELD_ITEM: u32 = 0x148;
/// The hit data's health damage and weapon.
const HIT_HEALTH_DAMAGE: u32 = 0x14;
const HIT_WEAPON: u32 = 0x30;

/// `TESObjectWEAP`: weight (`TESWeightForm`), damage (`TESAttackDamageForm`, 16 bits), and in
/// its `DATA` (`+0xF4`) the type, flags and projectiles.
const WEAPON_WEIGHT: u32 = 0x90;
const WEAPON_DAMAGE: u32 = 0xA0;
const WEAPON_TYPE: u32 = 0xF4;
const WEAPON_FLAGS: u32 = 0xF4 + 0x0C;
const WEAPON_PROJECTILES: u32 = 0xF4 + 0x2A;
/// The automatic flag of `cFlags`.
const FLAG_AUTOMATIC: u8 = 2;

/// What a weapon form gives the feel (none for no form).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeaponStats {
    /// `eType`: 0 to 2 melee, 3 to 9 guns, 10 to 13 thrown and placed.
    pub kind: i32,
    pub damage: f32,
    pub projectiles: u32,
    pub weight: f32,
    pub automatic: bool,
}

impl WeaponStats {
    pub fn is_melee(&self) -> bool {
        (0..=2).contains(&self.kind)
    }

    pub fn is_gun(&self) -> bool {
        (3..=9).contains(&self.kind)
    }
}

/// The stats of weapon form `weapon` (0: none).
fn weapon_stats(e: &Engine, weapon: u32) -> Option<WeaponStats> {
    if weapon == 0 {
        return None;
    }
    Some(WeaponStats {
        kind: i32::from(e.mem.i8(weapon + WEAPON_TYPE)),
        damage: f32::from(e.mem.u16(weapon + WEAPON_DAMAGE)),
        projectiles: u32::from(e.mem.u8(weapon + WEAPON_PROJECTILES)).max(1),
        weight: sane(e.mem.f32(weapon + WEAPON_WEIGHT), 0.0, 200.0),
        automatic: e.mem.u8(weapon + WEAPON_FLAGS) & FLAG_AUTOMATIC != 0,
    })
}

/// `value` inside `low..=high`, `low` for NaN.
fn sane(value: f32, low: f32, high: f32) -> f32 {
    if value.is_nan() {
        low
    } else {
        value.clamp(low, high)
    }
}

/// The force of a hit with the weapon: a gun's damage of all its projectiles (its calibre), a
/// melee weapon's damage made heavier by its weight (a 10-pound weapon doubles it).
pub fn force(stats: &WeaponStats) -> f32 {
    if stats.is_melee() {
        stats.damage * (1.0 + stats.weight / 10.0)
    } else {
        stats.damage * stats.projectiles.min(12) as f32
    }
}

/// The view kick of a shot, in degrees, before `iRecoil`: grows with the force and stops at 9; a
/// one-handed pistol flips a little more, a launcher more again, automatic fire and a held
/// trigger (type 8, the minigun and flamer kind) much less, and iron sights steady it.
pub fn kick_degrees(stats: &WeaponStats, sights: bool) -> f32 {
    if !stats.is_gun() {
        return 0.0;
    }
    let base = (0.35 + 0.055 * force(stats)).min(9.0);
    let kind = match stats.kind {
        3 | 4 => 1.15,
        8 => 0.25,
        9 => 1.4,
        _ => 1.0,
    };
    let auto = if stats.automatic || stats.kind == 6 {
        0.45
    } else {
        1.0
    };
    let steady = if sights { 0.7 } else { 1.0 };
    base * kind * auto * steady
}

/// How much harder a kill with the weapon pushes the body, before `iImpulse`: melee by weight
/// (0.75 for a light knife up to 2.5 for the heaviest), guns as the game has it (their damage
/// already scales the push).
pub fn impulse_scale(stats: Option<&WeaponStats>) -> f32 {
    match stats {
        Some(s) if s.is_melee() => (0.75 + s.weight / 12.0).clamp(0.75, 2.5),
        _ => 1.0,
    }
}

// --- The impact punch (no time dilation). ----------------------------------------------------

/// A short punch of the viewmodel and the camera on an impact: it rises over the first fifth of
/// `seconds` and snaps back over the rest ([`punch_envelope`]). The camera's part returns to where
/// it started; game time is never touched.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Punch {
    /// The viewmodel pushed back towards the player (game units), its muzzle lifted (radians) and
    /// twisted sideways (radians).
    pub back: f32,
    pub lift: f32,
    pub twist: f32,
    /// The camera's pitch (radians, negative up) and heading (radians) at the punch's peak.
    pub camera_pitch: f32,
    pub camera_heading: f32,
    pub seconds: f32,
}

/// How long a punch lasts: `iHitStopMs` times `scale`, from 40 to 250 ms; `None` when the punch
/// is off (`bHitStop=0` or `iHitStopMs=0`).
fn punch_seconds(tuning: &Tuning, scale: f32) -> Option<f32> {
    if !tuning.hit_stop || tuning.hit_stop_ms <= 0 {
        return None;
    }
    Some((tuning.hit_stop_ms as f32 * scale / 1000.0).clamp(0.04, 0.25))
}

/// The punch of a melee hit that connects, by the weapon's weight (fists and a knife light, a
/// super sledge the heaviest). `side` (-1 to 1) picks the sideways direction.
pub fn melee_punch(tuning: &Tuning, stats: Option<&WeaponStats>, side: f32) -> Option<Punch> {
    let w = (stats.map_or(0.0, |s| s.weight) / 20.0).clamp(0.0, 1.0);
    let seconds = punch_seconds(tuning, 0.85 + 0.45 * w)?;
    Some(Punch {
        back: 0.8 + 1.6 * w,
        lift: (1.2 + 1.8 * w).to_radians(),
        twist: side * (0.6 + 0.8 * w).to_radians(),
        camera_pitch: -(0.3 + 0.6 * w).to_radians(),
        camera_heading: side * (0.25 + 0.35 * w).to_radians(),
        seconds,
    })
}

/// The firmer kick of a heavy shot (a force of 25 or more: a shotgun, a magnum, a rifle), growing
/// to a force of 110; lighter shots have only their recoil. Iron sights halve the viewmodel's part.
pub fn shot_punch(tuning: &Tuning, stats: &WeaponStats, sights: bool, side: f32) -> Option<Punch> {
    if !stats.is_gun() {
        return None;
    }
    let force = force(stats);
    if force < 25.0 {
        return None;
    }
    let h = ((force - 25.0) / 85.0).clamp(0.0, 1.0);
    let seconds = punch_seconds(tuning, 0.8 + 0.5 * h)?;
    let model = if sights { 0.5 } else { 1.0 };
    Some(Punch {
        back: (1.0 + 2.0 * h) * model,
        lift: (1.0 + 2.5 * h).to_radians() * model,
        twist: side * (0.5 * h).to_radians() * model,
        camera_pitch: -(0.2 + 0.6 * h).to_radians(),
        camera_heading: side * (0.3 * h).to_radians(),
        seconds,
    })
}

/// The punch's share at `t` seconds of `seconds`: a quick rise to 1 over the first fifth, then an
/// ease back to 0 by the end (0 before and after).
pub fn punch_envelope(t: f32, seconds: f32) -> f32 {
    if t.is_nan() || t <= 0.0 || seconds <= 0.0 || t >= seconds {
        return 0.0;
    }
    let rise = 0.2 * seconds;
    if t < rise {
        (t / rise * std::f32::consts::FRAC_PI_2).sin()
    } else {
        let u = (t - rise) / (seconds - rise);
        (1.0 - u) * (1.0 - u)
    }
}

/// A punch under way: its time so far and the share of its camera part already turned in.
#[derive(Clone, Copy, Debug)]
struct Active {
    punch: Punch,
    elapsed: f32,
    camera_share: f32,
}

// --- The state of play (the player's frame, one at a time). --------------------------------

/// The kick to turn in, the share to give back, the punches and the sway.
#[derive(Debug, Default)]
struct State {
    /// Radians still to turn up and sideways.
    pending_pitch: f32,
    pending_heading: f32,
    /// Radians turned up that are still to come back down.
    debt: f32,
    /// The pitch the last frame ended on, to tell the player's own movement.
    last_pitch: Option<f32>,
    last_heading: Option<f32>,
    last_position: Option<[f32; 3]>,
    /// The punches under way (at most four).
    punches: Vec<Active>,
    sway: Sway,
    /// The viewmodel's pose for this frame's first-person draw, with the view's heading and pitch;
    /// `None` when there is nothing to show (third person, the Pip-Boy, V.A.T.S., off).
    pose: Option<(Pose, f32, f32)>,
    /// A small random state for the sideways kick.
    seed: u32,
}

/// The viewmodel's sway: two springs, one for its lag angles (yaw, pitch, roll, radians, in the
/// view's axes: about up, right and forward) and one for its offset (right, forward, up, game
/// units), the push back of a shot, and the walking bob's phase.
#[derive(Debug, Default)]
pub struct Sway {
    angle: [f32; 3],
    angle_velocity: [f32; 3],
    offset: [f32; 3],
    offset_velocity: [f32; 3],
    /// The push back of a shot, decaying.
    push: f32,
    phase: f32,
}

/// Where the viewmodel is drawn this frame relative to where the game puts it: lag angles (yaw,
/// pitch, roll, radians, about the view's up, right and forward, turned about the eye) and an
/// offset (right, forward, up, game units).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub angles: [f32; 3],
    pub offset: [f32; 3],
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(|| State {
        seed: 0x9e37_79b9,
        ..State::default()
    }))
}

/// A number from -1 to 1.
fn wobble(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (*seed as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Starts a punch (the oldest of four under way gives way).
fn start_punch(s: &mut State, punch: Punch) {
    if s.punches.len() >= 4 {
        s.punches.remove(0);
    }
    s.punches.push(Active {
        punch,
        elapsed: 0.0,
        camera_share: 0.0,
    });
}

// --- The hooks. ------------------------------------------------------------------------------

/// Registers the hooks and the Settings rows.
pub fn install(e: &mut Engine) {
    ON.store(true, Ordering::Relaxed);
    e.register(WEAPON_FIRE, |e, a| {
        fn_00523150(e, Ptr::new(a[0]), Ptr::new(a[1]));
        if on() && a[0] != 0 && a[1] != 0 && a[1] == e.global::<u32>(PLAYER) {
            shot(e, a[0], a[1]);
        }
        Ret::default()
    });
    e.register(LOOK_INPUT, |e, a| {
        let used = fn_009445b0(
            e,
            Ptr::new(a[0]),
            f32::from_bits(a[1]),
            a[2] & 0xff != 0,
            Ptr::new(a[3]),
            a[4],
        );
        if a[0] != 0 {
            frame(e, a[0], f32::from_bits(a[1]));
        }
        Ret {
            eax: u32::from(used),
            ..Ret::default()
        }
    });
    e.register(FIRST_PERSON_SETUP, |e, a| {
        let placed = if on() { place_pose(e) } else { None };
        main_draw_world_mt_setup_1st_person(e, Ptr::new(a[0]));
        if let Some(placed) = placed {
            unplace_pose(e, &placed);
        }
        Ret::default()
    });
    e.register(REACH_SEARCH, |e, a| {
        let mut reach = f32::from_bits(a[1]);
        if on() && a[0] != 0 && a[0] == e.global::<u32>(PLAYER) {
            reach *= tuning().melee_reach as f32 / 100.0;
        }
        fn_009a60e0(e, a[0], reach).into_ret()
    });
    e.register(COMBAT_HIT, |e, a| {
        actor_combat_hit(
            e,
            Ptr::new(a[0]),
            Ptr::new(a[1]),
            a[2] as u8,
            Ptr::new(a[3]),
            a[4] as u8,
        );
        if on()
            && a[0] != 0
            && a[1] != 0
            && a[0] == e.global::<u32>(PLAYER)
            && e.mem.i32(VATS_MODE) == 0
        {
            let weapon = held_weapon(e, a[0]);
            let stats = weapon_stats(e, weapon);
            if stats.map_or(true, |s| s.is_melee()) {
                with_state(|s| {
                    let side = wobble(&mut s.seed);
                    if let Some(punch) = melee_punch(&tuning(), stats.as_ref(), side) {
                        start_punch(s, punch);
                    }
                });
            }
        }
        Ret::default()
    });
    e.register(KILL_IMPULSE, |e, a| {
        kill_impulse(e, a[0], a[1]);
        Ret::default()
    });
    e.register(DISMEMBER_ROLL, |e, a| {
        fn_00646e50(
            e,
            a[0],
            Ptr::new(a[1]),
            a[2] as i32,
            a[3] as i32,
            a[4] as i32,
            a[5] & 0xff != 0,
            Ptr::new(a[6]),
            Ptr::new(a[7]),
        );
        if on() {
            // The part's explode chance (the third word) is 0 for a limb that never explodes.
            forced_outcome(e, a[1], a[2] as i32 > 0, a[6], a[7]);
        }
        Ret::default()
    });
    e.register(CREATE_MENU_ELEMENTS, |e, _| {
        start_menu_create_menu_elements(e);
        add_rows(e);
        Ret::default()
    });
    install_change_fns(e);
}

/// The weapon form the actor holds (0 for its fists or none).
fn held_weapon(e: &mut Engine, actor: u32) -> u32 {
    let process = e.mem.u32(actor + ACTOR_PROCESS);
    if process == 0 {
        return 0;
    }
    let item = e.vcall(process, PROCESS_HELD_ITEM, &args![]).u32();
    if item == 0 {
        0
    } else {
        e.mem.u32(item + 8)
    }
}

/// A shot of the player's: the kick to turn in, the weapon's push and, for a heavy shot, its punch.
fn shot(e: &mut Engine, weapon: u32, player: u32) {
    if e.mem.i32(VATS_MODE) != 0 {
        return;
    }
    let Some(stats) = weapon_stats(e, weapon) else {
        return;
    };
    let sights = e.call(ACTOR_GET_IRON_SIGHTS, &args![player]).bool();
    let tuning = tuning();
    let degrees = kick_degrees(&stats, sights) * tuning.recoil as f32 / 100.0;
    with_state(|s| {
        if degrees > 0.0 {
            let kick = degrees.to_radians();
            s.pending_pitch = (s.pending_pitch + kick).min(0.35);
            s.pending_heading += kick * 0.3 * wobble(&mut s.seed);
            s.sway.push = (s.sway.push + degrees * 0.35).min(3.0);
        }
        let side = wobble(&mut s.seed);
        if let Some(punch) = shot_punch(&tuning, &stats, sights, side) {
            start_punch(s, punch);
        }
    });
}

/// The kill impulse with its strength scaled (module documentation).
fn kill_impulse(e: &mut Engine, aggressor: u32, hit: u32) {
    if !on() || hit == 0 {
        fn_008ae000(e, Ptr::new(aggressor), Ptr::new(hit));
        return;
    }
    let tuning = tuning();
    let stats = weapon_stats(e, e.mem.u32(hit + HIT_WEAPON));
    let scale = tuning.impulse as f32 / 100.0 * impulse_scale(stats.as_ref());
    let damage = e.mem.f32(hit + HIT_HEALTH_DAMAGE);
    e.mem.set_f32(hit + HIT_HEALTH_DAMAGE, damage * scale);
    fn_008ae000(e, Ptr::new(aggressor), Ptr::new(hit));
    e.mem.set_f32(hit + HIT_HEALTH_DAMAGE, damage);
}

/// What the roll should say for a weapon of `stats`, given what it said (`sever`, `explode`):
/// unchanged when it said either; else sever from `iDismemberForce`, and for a gun explode an
/// explodable limb from `iExplodeForce`.
pub fn outcome(
    tuning: &Tuning,
    stats: &WeaponStats,
    explodable: bool,
    sever: bool,
    explode: bool,
) -> (bool, bool) {
    if sever || explode {
        return (sever, explode);
    }
    let force = force(stats);
    if explodable
        && stats.is_gun()
        && tuning.explode_force > 0
        && force >= tuning.explode_force as f32
    {
        return (false, true);
    }
    if tuning.dismember_force > 0 && force >= tuning.dismember_force as f32 {
        return (true, false);
    }
    (false, false)
}

/// The roll's results forced by the weapon's force, then the weapon's own override as the roll
/// applies it.
fn forced_outcome(e: &mut Engine, weapon: u32, explodable: bool, sever_at: u32, explode_at: u32) {
    let Some(stats) = weapon_stats(e, weapon) else {
        return;
    };
    let sever = e.mem.u8(sever_at) != 0;
    let explode = e.mem.u8(explode_at) != 0;
    let (new_sever, new_explode) = outcome(&tuning(), &stats, explodable, sever, explode);
    if (new_sever, new_explode) == (sever, explode) {
        return;
    }
    let (new_sever, new_explode) = match e.call(ROLL_OVERRIDE, &args![weapon]).u32() {
        1 => (true, false),
        2 => (false, true),
        3 => (false, false),
        _ => (new_sever, new_explode),
    };
    e.mem.set_u8(sever_at, u8::from(new_sever));
    e.mem.set_u8(explode_at, u8::from(new_explode));
}

/// Once a frame of the player's look handling: the kick and its recovery, the punches' camera
/// part, and the viewmodel's pose for the first-person draw ([`place_pose`]).
fn frame(e: &mut Engine, player: u32, time_step: f32) {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(|| State {
        seed: 0x9e37_79b9,
        ..State::default()
    });
    let dt = if time_step.is_finite() {
        time_step.clamp(0.0, 0.25)
    } else {
        0.0
    };
    if !on() || e.mem.i32(VATS_MODE) != 0 || dt <= 0.0 {
        s.pending_pitch = 0.0;
        s.pending_heading = 0.0;
        s.debt = 0.0;
        s.last_pitch = None;
        s.last_heading = None;
        s.last_position = None;
        s.punches.clear();
        s.sway = Sway::default();
        s.pose = None;
        return;
    }
    let tuning = tuning();
    // The player's own turn since the last frame (before this frame's kick and punch).
    let pitch = e.call(GET_LOOKING, &args![player]).f64() as f32;
    let heading = e.vcall(player, HEADING_SLOT, &args![0u32]).f64() as f32;
    let turned_pitch = s.last_pitch.map_or(0.0, |last| pitch - last);
    let turned_heading = s
        .last_heading
        .map_or(0.0, |last| wrap_angle(heading - last));
    // Pulling down while the kick comes back is the recovery done by hand.
    if turned_pitch > 0.0 {
        s.debt = (s.debt - turned_pitch).max(0.0);
    }
    let (up, side, back) = recoil_step(
        &mut s.pending_pitch,
        &mut s.pending_heading,
        &mut s.debt,
        tuning.recovery,
        dt,
    );
    let (punch_pitch, punch_heading, model) = punch_step(&mut s.punches, dt);
    let turn_pitch = back - up + punch_pitch;
    let turn_heading = side + punch_heading;
    if turn_pitch != 0.0 {
        e.call(ADD_LOOKING, &args![player, turn_pitch]);
    }
    if turn_heading != 0.0 {
        e.call(ADD_HEADING, &args![player, turn_heading]);
    }
    let pitch_after = e.call(GET_LOOKING, &args![player]).f64() as f32;
    let heading_after = e.vcall(player, HEADING_SLOT, &args![0u32]).f64() as f32;
    s.last_pitch = Some(pitch_after);
    s.last_heading = Some(heading_after);
    // The viewmodel's pose, in first person only.
    let position = [
        e.mem.f32(player + POSITION),
        e.mem.f32(player + POSITION + 4),
        e.mem.f32(player + POSITION + 8),
    ];
    let moved = s.last_position.map_or(0.0, |last| {
        let d = ((position[0] - last[0]).powi(2) + (position[1] - last[1]).powi(2)).sqrt();
        if d > 200.0 {
            0.0
        } else {
            d / dt
        }
    });
    s.last_position = Some(position);
    let shown = e.mem.u8(player + THIRD_PERSON) == 0
        && !e.call(PLAYER_IS_PIPBOY_ACTIVE, &args![player]).bool();
    if !shown {
        s.sway = Sway::default();
        s.pose = None;
        return;
    }
    let sights = e.call(ACTOR_GET_IRON_SIGHTS, &args![player]).bool();
    let weapon = held_weapon(e, player);
    let weight = weapon_stats(e, weapon).map_or(0.0, |w| w.weight);
    let strength = sway_strength(tuning.sway, weight, sights);
    let recoil = tuning.recoil as f32 / 100.0;
    let sway = sway_step(
        &mut s.sway,
        turned_heading / dt,
        turned_pitch / dt,
        moved,
        strength,
        recoil,
        dt,
    );
    let pose = Pose {
        angles: [
            sway.angles[0] + model.angles[0],
            sway.angles[1] + model.angles[1],
            sway.angles[2] + model.angles[2],
        ],
        offset: [
            sway.offset[0] + model.offset[0],
            sway.offset[1] + model.offset[1],
            sway.offset[2] + model.offset[2],
        ],
    };
    s.pose = (pose != Pose::default()).then_some((pose, heading_after, pitch_after));
}

/// The sway's strength: `iSway` percent, more for a heavier weapon (0.8 for fists and a pistol up
/// to 1.5 for the heaviest), a fifth in iron sights; 0 with `iSway=0`.
pub fn sway_strength(sway: i32, weight: f32, sights: bool) -> f32 {
    let heavy = (0.8 + sane(weight, 0.0, 200.0) / 25.0).clamp(0.8, 1.5);
    sway.max(0) as f32 / 100.0 * heavy * if sights { 0.2 } else { 1.0 }
}

/// One frame of the punches: the camera's turn this frame (pitch, heading, radians), so each
/// punch's camera part comes back to where it started, and the viewmodel's share of them all.
/// Finished punches are dropped.
fn punch_step(punches: &mut Vec<Active>, dt: f32) -> (f32, f32, Pose) {
    let mut pitch = 0.0;
    let mut heading = 0.0;
    let mut model = Pose::default();
    for active in punches.iter_mut() {
        active.elapsed += dt;
        let share = punch_envelope(active.elapsed, active.punch.seconds);
        let change = share - active.camera_share;
        active.camera_share = share;
        pitch += change * active.punch.camera_pitch;
        heading += change * active.punch.camera_heading;
        // A lifted muzzle is a positive angle about the view's right axis.
        model.angles[0] += share * active.punch.twist;
        model.angles[1] += share * active.punch.lift;
        model.offset[1] -= share * active.punch.back;
    }
    punches.retain(|a| a.elapsed < a.punch.seconds);
    (pitch, heading, model)
}

/// An angle difference inside -pi..pi.
fn wrap_angle(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut a = a % tau;
    if a > std::f32::consts::PI {
        a -= tau;
    } else if a < -std::f32::consts::PI {
        a += tau;
    }
    a
}

/// One frame of the kick: how far to turn up (radians), sideways, and back down. The kick comes
/// in over about 35 ms; `recovery` percent of it is owed back and, once the kick is mostly in,
/// comes back over about 160 ms.
pub fn recoil_step(
    pending_pitch: &mut f32,
    pending_heading: &mut f32,
    debt: &mut f32,
    recovery: i32,
    dt: f32,
) -> (f32, f32, f32) {
    let take = 1.0 - (-dt / 0.035).exp();
    let mut up = *pending_pitch * take;
    let mut side = *pending_heading * take;
    if pending_pitch.abs() < 1e-4 {
        up = *pending_pitch;
    }
    if pending_heading.abs() < 1e-4 {
        side = *pending_heading;
    }
    *pending_pitch -= up;
    *pending_heading -= side;
    *debt += up * recovery.clamp(0, 100) as f32 / 100.0;
    let mut back = 0.0;
    if *pending_pitch < 0.25 * up.max(1e-3) || *pending_pitch < 1e-3 {
        back = *debt * (1.0 - (-dt / 0.16).exp());
        if *debt < 1e-4 {
            back = *debt;
        }
        *debt -= back;
    }
    (up, side, back)
}

/// A spring towards `target`: about 2.4 swings a second, a little under critical damping, so the
/// weapon settles back with a slight give. Stepped in pieces of at most 1/240 s.
fn spring(value: &mut f32, velocity: &mut f32, target: f32, dt: f32) {
    const OMEGA: f32 = 2.4 * std::f32::consts::TAU;
    const DAMPING: f32 = 0.72;
    let steps = (dt * 240.0).ceil().max(1.0) as u32;
    let h = dt / steps as f32;
    for _ in 0..steps {
        let accel = OMEGA * OMEGA * (target - *value) - 2.0 * DAMPING * OMEGA * *velocity;
        *velocity += accel * h;
        *value += *velocity * h;
    }
}

/// One frame of the sway: the weapon lags the turn (radians a second: it turns a little behind the
/// view and comes back), bobs with the walk (units a second) and is pushed back by a shot; the
/// viewmodel's pose from it. With no strength and no push it is the game's own place.
pub fn sway_step(
    sway: &mut Sway,
    heading_rate: f32,
    pitch_rate: f32,
    speed: f32,
    strength: f32,
    recoil: f32,
    dt: f32,
) -> Pose {
    let heading_rate = sane(heading_rate, -40.0, 40.0);
    let pitch_rate = sane(pitch_rate, -40.0, 40.0);
    // Turning right (the heading grows) leaves the weapon turned left: a positive angle about the
    // view's up axis. Looking down (the pitch grows) leaves its muzzle up: positive about the
    // right axis. A slight roll away from the turn.
    let angle_target = [
        (0.014 * heading_rate).clamp(-0.06, 0.06) * strength,
        (0.012 * pitch_rate).clamp(-0.045, 0.045) * strength,
        (-0.008 * heading_rate).clamp(-0.035, 0.035) * strength,
    ];
    let speed = sane(speed, 0.0, 600.0);
    sway.phase = (sway.phase + dt * speed * 0.03) % std::f32::consts::TAU;
    let bob = (speed / 300.0).min(1.0) * 0.3 * strength;
    let offset_target = [
        (-0.45 * heading_rate).clamp(-1.4, 1.4) * strength + bob * sway.phase.sin(),
        0.0,
        (0.4 * pitch_rate).clamp(-1.2, 1.2) * strength + bob * 0.5 * (2.0 * sway.phase).sin().abs()
            - bob * 0.25,
    ];
    for k in 0..3 {
        spring(
            &mut sway.angle[k],
            &mut sway.angle_velocity[k],
            angle_target[k],
            dt,
        );
        spring(
            &mut sway.offset[k],
            &mut sway.offset_velocity[k],
            offset_target[k],
            dt,
        );
    }
    if strength == 0.0 {
        *sway = Sway {
            push: sway.push,
            ..Sway::default()
        };
    }
    let pose = Pose {
        angles: sway.angle,
        offset: [
            sway.offset[0],
            sway.offset[1] - sway.push * recoil,
            sway.offset[2],
        ],
    };
    sway.push *= (-dt / 0.08).exp();
    if sway.push < 1e-3 {
        sway.push = 0.0;
    }
    pose
}

/// The view's right, forward and up axes in the world for `heading` (around z, 0 north,
/// clockwise) and `pitch` (positive down).
pub fn view_axes(heading: f32, pitch: f32) -> [[f32; 3]; 3] {
    let (sh, ch) = heading.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    [
        [ch, -sh, 0.0],
        [sh * cp, ch * cp, -sp],
        [sh * sp, ch * sp, cp],
    ]
}

/// The view's (right, forward, up) offset in the world.
pub fn view_to_world(offset: [f32; 3], heading: f32, pitch: f32) -> [f32; 3] {
    let [right, forward, up] = view_axes(heading, pitch);
    let mut world = [0.0; 3];
    for (i, w) in world.iter_mut().enumerate() {
        *w = right[i] * offset[0] + forward[i] * offset[1] + up[i] * offset[2];
    }
    world
}

/// A 3x3 matrix, rows first (`NiMatrix3`), and its products.
type Matrix = [[f32; 3]; 3];

fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

fn transpose(a: &Matrix) -> Matrix {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = a[j][i];
        }
    }
    m
}

fn apply(a: &Matrix, v: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0; 3];
    for (i, o) in out.iter_mut().enumerate() {
        *o = (0..3).map(|k| a[i][k] * v[k]).sum();
    }
    out
}

/// The rotation by `angle` radians about the unit `axis` (right-handed).
fn about(axis: [f32; 3], angle: f32) -> Matrix {
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    let [x, y, z] = axis;
    [
        [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
        [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
        [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
    ]
}

/// The pose in the world: the rotation (about the view's up, right and forward axes, in that
/// order) and the translation that, applied to the first-person root, turns the viewmodel about
/// the eye (`eye`, from the root) and moves it by the offset.
pub fn pose_in_world(pose: &Pose, heading: f32, pitch: f32, eye: [f32; 3]) -> (Matrix, [f32; 3]) {
    let [right, forward, up] = view_axes(heading, pitch);
    let rotation = mul(
        &mul(&about(up, pose.angles[0]), &about(right, pose.angles[1])),
        &about(forward, pose.angles[2]),
    );
    let turned = apply(&rotation, eye);
    let moved = view_to_world(pose.offset, heading, pitch);
    let mut translation = [0.0; 3];
    for k in 0..3 {
        translation[k] = moved[k] + eye[k] - turned[k];
    }
    (rotation, translation)
}

/// What [`place_pose`] changed, to put back after the game's first-person set-up.
struct Placed {
    node: u32,
    local_rotation: [f32; 9],
    world_translation: [f32; 3],
    written_translation: [f32; 3],
}

fn read_floats<const N: usize>(e: &Engine, at: u32) -> [f32; N] {
    let mut out = [0.0; N];
    for (k, value) in out.iter_mut().enumerate() {
        *value = e.mem.f32(at + 4 * k as u32);
    }
    out
}

fn write_floats(e: &Engine, at: u32, values: &[f32]) {
    for (k, value) in values.iter().enumerate() {
        e.mem.set_f32(at + 4 * k as u32, *value);
    }
}

fn matrix_of(m: &[f32; 9]) -> Matrix {
    [[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]]
}

/// Before `Main::DrawWorld_MTSetup1stPerson` (`00874c10`): the first-person root gets the pose.
/// That function zeroes the root's local translation, updates the root and everything under it
/// (`00a59c60`), puts the translation back, and places the first-person camera at the camera node
/// (`011e07d0`) less the root's world translation (`+0x8c`). So the lag rotation goes into the
/// root's local rotation (turned about the eye) and the offset into the world translation the
/// camera is placed from; the game's own update carries them to the arms and the weapon, after
/// every animation of the frame and just before they are drawn. [`unplace_pose`] puts the local
/// rotation back, so nothing builds up.
fn place_pose(e: &mut Engine) -> Option<Placed> {
    let (pose, heading, pitch) = with_state(|s| s.pose)?;
    let player = e.global::<u32>(PLAYER);
    if player == 0 {
        return None;
    }
    let node = e.call(GET_PLAYER_NODE, &args![player, 1u32]).u32();
    if node == 0 {
        return None;
    }
    let local_rotation: [f32; 9] = read_floats(e, node + LOCAL_ROTATION);
    let world_translation: [f32; 3] = read_floats(e, node + WORLD_TRANSLATION);
    if local_rotation
        .iter()
        .chain(&world_translation)
        .any(|v| !v.is_finite())
    {
        return None;
    }
    let camera = e.mem.u32(CAMERA_NODE);
    let eye = if camera == 0 {
        [0.0; 3]
    } else {
        let at: [f32; 3] = read_floats(e, camera + WORLD_TRANSLATION);
        [
            at[0] - world_translation[0],
            at[1] - world_translation[1],
            at[2] - world_translation[2],
        ]
    };
    // An eye far from the root is no first-person set-up this works for: leave it alone.
    if eye.iter().any(|v| !v.is_finite() || v.abs() > 512.0) {
        return None;
    }
    let (rotation, translation) = pose_in_world(&pose, heading, pitch, eye);
    let parent = e.mem.u32(node + PARENT);
    let turn = if parent == 0 {
        rotation
    } else {
        let p = matrix_of(&read_floats(e, parent + WORLD_ROTATION));
        mul(&mul(&transpose(&p), &rotation), &p)
    };
    let local = mul(&turn, &matrix_of(&local_rotation));
    let written = [
        world_translation[0] + translation[0],
        world_translation[1] + translation[1],
        world_translation[2] + translation[2],
    ];
    write_floats(e, node + LOCAL_ROTATION, &local.concat());
    write_floats(e, node + WORLD_TRANSLATION, &written);
    Some(Placed {
        node,
        local_rotation,
        world_translation,
        written_translation: written,
    })
}

/// After the set-up: the root's local rotation as the game had it, and its world translation too
/// where the set-up did not update the root (the update rewrites it otherwise).
fn unplace_pose(e: &mut Engine, placed: &Placed) {
    write_floats(e, placed.node + LOCAL_ROTATION, &placed.local_rotation);
    let now: [f32; 3] = read_floats(e, placed.node + WORLD_TRANSLATION);
    if now == placed.written_translation {
        write_floats(
            e,
            placed.node + WORLD_TRANSLATION,
            &placed.world_translation,
        );
    }
}

// --- The Settings menu's rows. ---------------------------------------------------------------

/// The rows' change functions, after the graphics rows' (`0x7e06_0010` to `0x7e06_0050`), in the
/// platform's area that no exe or platform code uses.
pub const CHANGE_FNS: [u32; 5] = [
    0x7e06_0060,
    0x7e06_0070,
    0x7e06_0080,
    0x7e06_0090,
    0x7e06_00a0,
];

/// A row of the Settings menu's Display page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// Off / On: the whole row.
    Enabled,
    /// A meter of `iRecoil`.
    Recoil,
    /// A meter of `iSway`.
    Sway,
    /// Off / On: `bHitStop` (the impact punch).
    HitStop,
    /// A meter of `iDismemberForce` (0 Off).
    Dismember,
}

/// The rows, in the order they are made.
pub const ROWS: [Row; 5] = [
    Row::Enabled,
    Row::Recoil,
    Row::Sway,
    Row::HitStop,
    Row::Dismember,
];

impl Row {
    /// The Fallout.ini section and key the row saves to.
    pub fn key(self) -> (&'static str, &'static str) {
        match self {
            Row::Enabled => (crate::game_ini::FIXES_SECTION, "bWeightyCombat"),
            Row::Recoil => (SECTION, "iRecoil"),
            Row::Sway => (SECTION, "iSway"),
            Row::HitStop => (SECTION, "bHitStop"),
            Row::Dismember => (SECTION, "iDismemberForce"),
        }
    }

    /// The meter's step and number of settings; `None` for an Off / On list.
    pub fn meter(self) -> Option<(i32, u32)> {
        match self {
            Row::Recoil | Row::Sway => Some((10, 21)),
            Row::Dismember => Some((5, 31)),
            Row::Enabled | Row::HitStop => None,
        }
    }

    /// The row's value now.
    pub fn value(self) -> i32 {
        match self {
            Row::Enabled => i32::from(on()),
            row => tuning().get(row.key().1).unwrap_or(0),
        }
    }

    /// The row's value when the menu's Defaults is chosen.
    pub fn default_value(self) -> i32 {
        match self {
            Row::Enabled => 1,
            row => Tuning::PLAY.get(row.key().1).unwrap_or(0),
        }
    }

    /// The setting (meter step or list index) of `value`, and the value of a setting.
    pub fn setting_of(self, value: i32) -> u32 {
        match self.meter() {
            Some((step, count)) => {
                ((value.max(0) + step / 2) / step).clamp(0, count as i32 - 1) as u32
            }
            None => u32::from(value != 0),
        }
    }

    pub fn value_of(self, setting: u32) -> i32 {
        match self.meter() {
            Some((step, count)) => setting.min(count - 1) as i32 * step,
            None => i32::from(setting != 0),
        }
    }

    /// The row's label for `value`.
    pub fn label(self, value: i32) -> String {
        match self {
            Row::Enabled => "Weighty Combat".to_string(),
            Row::Recoil => format!("Recoil: {value}%"),
            Row::Sway => format!("Weapon Sway: {value}%"),
            Row::HitStop => "Impact Punch".to_string(),
            Row::Dismember if value == 0 => "Dismember Force: Off".to_string(),
            Row::Dismember => format!("Dismember Force: {value}"),
        }
    }
}

/// `pUserOptionsA`, its `Add`, `operator new`, `StartMenu::UserOption::UserOption` and a user
/// option's words, as [`crate::fov_slider`] has them.
const USER_OPTIONS: u32 = 0x011d_ab50;
const ARRAY_ADD: u32 = 0x007c_b2e0;
const OPERATOR_NEW: u32 = 0x0040_1000;
const USER_OPTION_NEW: u32 = 0x007d_6350;
const USER_OPTION_SIZE: u32 = 0x30;
const TEXT: u32 = 4;
const VAL: u32 = 0x14;
const DEFAULT_VAL: u32 = 0x18;
/// The Display page, a list (values between arrows) and a meter, and the label's buffer.
const DISPLAY: u32 = 0x10;
const LIST: u32 = 0;
const METER: u32 = 2;
const LABEL_SIZE: u32 = 32;
/// `StartMenu::pMe`, the `uiFlags` test, the save flag, the options list, `GetTileFor` and
/// `Tile::SetString` with the string trait.
const START_MENU: u32 = 0x011d_aac0;
const MENU_FLAGS_TEST: u32 = 0x004a_4080;
const FLAG_SAVE_OPTIONS: u32 = 0x20;
const OPTIONS_LIST: u32 = 0x114;
const GET_TILE_FOR: u32 = 0x007a_22d0;
const TILE_SET_STRING: u32 = 0x00a0_1350;
const TRAIT_STRING: u32 = 4036;

/// A string of the game's heap holding `text`, 0 when none could be had.
fn heap_text(e: &mut Engine, text: &str, size: u32) -> u32 {
    let buffer = e.call(OPERATOR_NEW, &args![size]).u32();
    if buffer != 0 {
        let bytes = &text.as_bytes()[..text.len().min(size as usize - 1)];
        e.mem.set_cstr(buffer, bytes);
    }
    buffer
}

/// Makes the rows and appends them to `pUserOptionsA` (a failed allocation appends nothing).
pub fn add_rows(e: &mut Engine) {
    for (row, change_fn) in ROWS.into_iter().zip(CHANGE_FNS) {
        let text = heap_text(e, &row.label(row.value()), LABEL_SIZE);
        let block = e.call(OPERATOR_NEW, &args![USER_OPTION_SIZE]).u32();
        if text == 0 || block == 0 {
            continue;
        }
        let mut call = match row.meter() {
            Some((_, count)) => args![block, text, change_fn, DISPLAY, METER, count, 0u32],
            None => args![block, text, change_fn, DISPLAY, LIST, 2u32, 0u32],
        };
        if row.meter().is_none() {
            let off = heap_text(e, "Off", 8);
            let on = heap_text(e, "On", 8);
            if off == 0 || on == 0 {
                continue;
            }
            call.extend([off, on]);
        }
        let option = e.call(USER_OPTION_NEW, &call).u32();
        e.with_stack(4, |e, cell| {
            e.mem.set_u32(cell.addr(), option);
            e.call(ARRAY_ADD, &args![USER_OPTIONS, cell]);
        });
    }
}

/// Registers the rows' change functions.
pub fn install_change_fns(e: &mut Engine) {
    fn row_fn<const N: usize>(e: &mut Engine, a: &[u32]) -> Ret {
        change(e, ROWS[N], Ptr::new(a[0]));
        Ret::default()
    }
    let fns: [AbiFn; 5] = [
        row_fn::<0>,
        row_fn::<1>,
        row_fn::<2>,
        row_fn::<3>,
        row_fn::<4>,
    ];
    for (address, f) in CHANGE_FNS.into_iter().zip(fns) {
        e.register(address, f);
    }
}

/// A row's change function: with the save flag clear the menu reads the value; set, the player
/// moved the row, which applies at once and is saved.
fn change(e: &mut Engine, row: Row, option: Ptr) {
    if option.is_null() {
        return;
    }
    let menu = e.global::<u32>(START_MENU);
    let saving = menu != 0
        && e.call(MENU_FLAGS_TEST, &args![menu, FLAG_SAVE_OPTIONS])
            .u32()
            & 0xff
            != 0;
    if !saving {
        e.mem
            .set_u32(option.addr() + VAL, row.setting_of(row.value()));
        e.mem.set_u32(
            option.addr() + DEFAULT_VAL,
            row.setting_of(row.default_value()),
        );
        write_label(e, 0, option, row.label(row.value()));
        return;
    }
    let value = row.value_of(e.mem.u32(option.addr() + VAL));
    if value == row.value() {
        return;
    }
    match row {
        Row::Enabled => {
            ON.store(value != 0, Ordering::Relaxed);
        }
        row => {
            let mut tuning = tuning();
            tuning.set(row.key().1, value);
            set_tuning(tuning);
        }
    }
    write_label(e, menu, option, row.label(value));
    let saved = save(row, value);
    eprintln!(
        "Weighty combat: {} {value}: applied now{}",
        row.key().1,
        if saved { ", saved" } else { ", not saved" }
    );
}

/// The label into the row's text and, with the menu, its tile.
fn write_label(e: &mut Engine, menu: u32, option: Ptr, label: String) {
    let text = e.mem.u32(option.addr() + TEXT);
    if text == 0 {
        return;
    }
    let bytes = &label.as_bytes()[..label.len().min(LABEL_SIZE as usize - 1)];
    if e.mem.cstr(text) == bytes {
        return;
    }
    e.mem.set_cstr(text, bytes);
    if menu == 0 {
        return;
    }
    let tile = e.with_stack(4, |e, cell| {
        e.mem.set_u32(cell.addr(), option.addr());
        e.call(GET_TILE_FOR, &args![menu + OPTIONS_LIST, cell])
            .u32()
    });
    if tile != 0 {
        e.call(TILE_SET_STRING, &args![tile, TRAIT_STRING, text, 1u32]);
    }
}

/// The row's key into the run's Fallout.ini, in a launcher's play run only; false when not saved
/// (a file that can't be written is said once in the log).
fn save(row: Row, value: i32) -> bool {
    if !crate::fixes::is_play_run(|name| std::env::var(name).ok()) {
        return false;
    }
    let (section, key) = row.key();
    let text = value.to_string();
    let written = crate::game_ini::path()
        .ok_or_else(|| "no Fallout.ini for this run".to_string())
        .and_then(|path| {
            crate::game_ini::write_keys(&path, &[(section, key, text.as_str())])
                .map_err(|why| format!("{} ({why})", path.display()))
        });
    if let Err(why) = written {
        static NAMED: std::sync::Once = std::sync::Once::new();
        NAMED.call_once(|| eprintln!("Weighty combat: {why} not saved"));
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gun(kind: i32, damage: f32, projectiles: u32, automatic: bool) -> WeaponStats {
        WeaponStats {
            kind,
            damage,
            projectiles,
            weight: 4.0,
            automatic,
        }
    }

    fn melee(damage: f32, weight: f32) -> WeaponStats {
        WeaponStats {
            kind: 2,
            damage,
            projectiles: 1,
            weight,
            automatic: false,
        }
    }

    /// The table's [nv-rs Gunplay] values are [`Tuning::PLAY`], and each key has its range.
    #[test]
    fn the_tables_gunplay_rows_are_the_built_in_tuning() {
        let rows: Vec<_> = crate::game_ini::rows()
            .into_iter()
            .filter(|r| r.section == SECTION)
            .collect();
        assert_eq!(
            rows.iter().map(|r| r.key).collect::<Vec<_>>(),
            KEYS.iter().map(|k| k.0).collect::<Vec<_>>()
        );
        let mut tuning = Tuning {
            recoil: 0,
            recovery: 0,
            sway: 0,
            hit_stop: false,
            hit_stop_ms: 0,
            melee_reach: 50,
            impulse: 0,
            dismember_force: 0,
            explode_force: 0,
        };
        for (row, key) in rows.iter().zip(KEYS) {
            assert_eq!(row.switch, key.1, "{row:?}");
            assert!(tuning.set(row.key, parse(row.key, row.default).unwrap()));
        }
        assert_eq!(tuning, Tuning::PLAY);
    }

    #[test]
    fn values_out_of_range_or_not_numbers_are_refused() {
        assert_eq!(parse("iRecoil", "150"), Some(150));
        assert_eq!(parse("iRecoil", "-1"), None);
        assert_eq!(parse("iRecoil", "lots"), None);
        assert_eq!(parse("bHitStop", "1"), Some(1));
        assert_eq!(parse("bHitStop", "2"), None);
        assert_eq!(parse("iMeleeReach", "20"), None);
        assert_eq!(parse("iNothing", "1"), None);
        let env = |name: &str| (name == "NV_GUNPLAY_RECOIL").then(|| "40".to_string());
        let tuning = Tuning::from_parts(&[("iRecoil", 150), ("iSway", 0)], env).unwrap();
        assert_eq!(tuning.recoil, 40);
        assert_eq!(tuning.sway, 0);
        let bad = |name: &str| (name == "NV_GUNPLAY_SWAY").then(|| "x".to_string());
        assert!(Tuning::from_parts(&[], bad)
            .unwrap_err()
            .contains("NV_GUNPLAY_SWAY=x"));
    }

    /// Bigger calibres kick harder; automatic fire, a held trigger and iron sights less; melee and
    /// thrown weapons not at all.
    #[test]
    fn the_kick_follows_the_calibre() {
        let pistol = kick_degrees(&gun(3, 20.0, 1, false), false);
        let magnum = kick_degrees(&gun(3, 48.0, 1, false), false);
        let anti_materiel = kick_degrees(&gun(5, 110.0, 1, false), false);
        let automatic = kick_degrees(&gun(6, 22.0, 1, true), false);
        let minigun = kick_degrees(&gun(8, 12.0, 1, true), false);
        assert!(pistol > 1.0 && pistol < 2.0, "{pistol}");
        assert!(magnum > pistol * 1.8, "{magnum}");
        assert!(
            anti_materiel > magnum && anti_materiel <= 9.0,
            "{anti_materiel}"
        );
        assert!(automatic < pistol, "{automatic}");
        assert!(minigun < automatic, "{minigun}");
        assert!(kick_degrees(&gun(3, 48.0, 1, false), true) < magnum);
        assert_eq!(kick_degrees(&melee(40.0, 10.0), false), 0.0);
        assert_eq!(kick_degrees(&gun(10, 100.0, 1, false), false), 0.0);
        // A shotgun's pellets count together.
        assert!(
            kick_degrees(&gun(5, 7.0, 8, false), false)
                > kick_degrees(&gun(5, 7.0, 1, false), false)
        );
    }

    /// The kick turns in within a tenth of a second and the recovery share comes back after.
    #[test]
    fn the_kick_comes_in_and_a_share_comes_back() {
        let (mut pitch, mut heading, mut debt) = (0.05f32, 0.01f32, 0.0f32);
        let (mut up_total, mut back_total) = (0.0, 0.0);
        for _ in 0..60 {
            let (up, _, back) = recoil_step(&mut pitch, &mut heading, &mut debt, 70, 1.0 / 60.0);
            up_total += up;
            back_total += back;
        }
        assert!((up_total - 0.05).abs() < 1e-4, "{up_total}");
        assert!((back_total - 0.035).abs() < 1e-3, "{back_total}");
        assert!(pitch.abs() < 1e-6 && heading.abs() < 1e-6);
        // None of it comes back with no recovery.
        let (mut pitch, mut heading, mut debt) = (0.05f32, 0.0f32, 0.0f32);
        let mut back_total = 0.0;
        for _ in 0..60 {
            back_total += recoil_step(&mut pitch, &mut heading, &mut debt, 0, 1.0 / 60.0).2;
        }
        assert_eq!(back_total, 0.0);
    }

    /// A fast turn leaves the weapon turned a few degrees behind the view, and it settles back
    /// once the turn stops; a shot pushes it back; `iSway=0` leaves it where the game puts it.
    #[test]
    fn the_sway_lags_the_turn_and_settles() {
        let strength = sway_strength(100, 4.0, false);
        let mut sway = Sway::default();
        let mut pose = Pose::default();
        // A quick turn to the right at 3 radians a second for a fifth of a second.
        for _ in 0..12 {
            pose = sway_step(&mut sway, 3.0, 0.0, 0.0, strength, 1.0, 1.0 / 60.0);
        }
        let yaw = pose.angles[0].to_degrees();
        assert!(
            yaw > 1.0 && yaw < 4.0,
            "the weapon turned left behind the view: {yaw}"
        );
        assert!(pose.offset[0] < -0.3, "{pose:?}");
        // Looking down leaves the muzzle up.
        let mut down = Sway::default();
        let mut lifted = Pose::default();
        for _ in 0..12 {
            lifted = sway_step(&mut down, 0.0, 2.0, 0.0, strength, 1.0, 1.0 / 60.0);
        }
        assert!(
            lifted.angles[1] > 0.0 && lifted.offset[2] > 0.0,
            "{lifted:?}"
        );
        // Settles back.
        for _ in 0..240 {
            pose = sway_step(&mut sway, 0.0, 0.0, 0.0, strength, 1.0, 1.0 / 60.0);
        }
        assert!(
            pose.angles
                .iter()
                .chain(&pose.offset)
                .all(|v| v.abs() < 1e-3),
            "{pose:?}"
        );
        // A shot pushes it back towards the player.
        sway.push = 1.0;
        let pushed = sway_step(&mut sway, 0.0, 0.0, 0.0, strength, 1.0, 1.0 / 60.0);
        assert!(pushed.offset[1] < 0.0);
        // iSway=0: no sway at all, whatever the turn.
        assert_eq!(sway_strength(0, 20.0, false), 0.0);
        let mut still = Sway::default();
        for _ in 0..30 {
            pose = sway_step(&mut still, 8.0, -6.0, 300.0, 0.0, 0.0, 1.0 / 60.0);
        }
        assert_eq!(pose, Pose::default());
        // Heavier weapons lag more, iron sights much less.
        assert!(sway_strength(100, 20.0, false) > sway_strength(100, 2.0, false));
        assert!(sway_strength(100, 4.0, true) < 0.3 * strength);
    }

    /// The first-person set-up's wrap: the root's local rotation gets the lag, its world translation
    /// the camera's offset; afterwards the local rotation is the game's again, and the world
    /// translation too when the game did not update the root.
    #[test]
    fn the_pose_goes_into_the_root_and_comes_out_again() {
        use std::sync::atomic::AtomicU32;
        static NODE: AtomicU32 = AtomicU32::new(0);
        let mut e = Engine::new();
        e.map(PLAYER & !0xfff, 0x1000);
        e.map(CAMERA_NODE & !0xfff, 0x1000);
        let player = e.mem.alloc(0x700);
        let node = e.mem.alloc(0xc0);
        let camera = e.mem.alloc(0xc0);
        e.mem.set_u32(PLAYER, player);
        e.mem.set_u32(CAMERA_NODE, camera);
        NODE.store(node, Ordering::Relaxed);
        e.register(GET_PLAYER_NODE, |_, _| {
            NODE.load(Ordering::Relaxed).into_ret()
        });
        let identity = [1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        write_floats(&e, node + LOCAL_ROTATION, &identity);
        write_floats(&e, node + WORLD_TRANSLATION, &[100.0, 200.0, 300.0]);
        write_floats(&e, camera + WORLD_TRANSLATION, &[100.0, 200.0, 420.0]);
        let pose = Pose {
            angles: [0.04, 0.0, 0.0],
            offset: [0.5, 0.0, 0.0],
        };
        with_state(|s| s.pose = Some((pose, 0.0, 0.0)));
        let placed = place_pose(&mut e).expect("placed");
        with_state(|s| s.pose = None);
        let local: [f32; 9] = read_floats(&e, node + LOCAL_ROTATION);
        assert_ne!(local, identity);
        let (rotation, translation) = pose_in_world(&pose, 0.0, 0.0, [0.0, 0.0, 120.0]);
        assert_eq!(local.to_vec(), rotation.concat());
        let world: [f32; 3] = read_floats(&e, node + WORLD_TRANSLATION);
        for k in 0..3 {
            assert!((world[k] - ([100.0, 200.0, 300.0][k] + translation[k])).abs() < 1e-3);
        }
        // Not updated by the game: both come back.
        unplace_pose(&mut e, &placed);
        assert_eq!(read_floats::<9>(&e, node + LOCAL_ROTATION), identity);
        assert_eq!(
            read_floats::<3>(&e, node + WORLD_TRANSLATION),
            [100.0, 200.0, 300.0]
        );
        // Updated by the game (its world translation rewritten): the game's stays.
        let placed = Placed {
            node,
            local_rotation: identity,
            world_translation: [1.0, 2.0, 3.0],
            written_translation: [9.0, 9.0, 9.0],
        };
        unplace_pose(&mut e, &placed);
        assert_eq!(
            read_floats::<3>(&e, node + WORLD_TRANSLATION),
            [100.0, 200.0, 300.0]
        );
        // No pose, no change.
        assert!(place_pose(&mut e).is_none());
    }

    /// The pose's rotation turns the viewmodel about the eye: the eye stays put, a point in front
    /// of it moves the way the angle says, and the offset is in the view's axes.
    #[test]
    fn the_pose_turns_about_the_eye() {
        let eye = [0.0, 0.0, 120.0];
        let pose = Pose {
            angles: [0.05, 0.0, 0.0],
            offset: [0.0; 3],
        };
        let (rotation, translation) = pose_in_world(&pose, 0.0, 0.0, eye);
        let place = |p: [f32; 3]| {
            let r = apply(&rotation, p);
            [
                r[0] + translation[0],
                r[1] + translation[1],
                r[2] + translation[2],
            ]
        };
        let at_eye = place(eye);
        assert!(
            at_eye.iter().zip(eye).all(|(a, b)| (a - b).abs() < 1e-3),
            "{at_eye:?}"
        );
        // A point 30 units ahead (north) turns left (west, negative x) about the eye.
        let ahead = place([0.0, 30.0, 120.0]);
        assert!(
            ahead[0] < -1.0 && (ahead[2] - 120.0).abs() < 1e-3,
            "{ahead:?}"
        );
        // The lift raises a point ahead.
        let lift = Pose {
            angles: [0.0, 0.05, 0.0],
            offset: [0.0; 3],
        };
        let (rotation, translation) = pose_in_world(&lift, 0.0, 0.0, eye);
        let r = apply(&rotation, [0.0, 30.0, 120.0]);
        assert!(r[2] + translation[2] > 121.0);
        // The offset alone moves along the view's axes (facing east: right is south).
        let push = Pose {
            angles: [0.0; 3],
            offset: [1.0, 0.0, 0.0],
        };
        let (_, moved) = pose_in_world(&push, std::f32::consts::FRAC_PI_2, 0.0, eye);
        assert!(
            (moved[1] + 1.0).abs() < 1e-5 && moved[0].abs() < 1e-5,
            "{moved:?}"
        );
    }

    #[test]
    fn view_axes_follow_heading_and_pitch() {
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
        assert!(close(
            view_to_world([0.0, 1.0, 0.0], 0.0, 0.0),
            [0.0, 1.0, 0.0]
        ));
        assert!(close(
            view_to_world([1.0, 0.0, 0.0], 0.0, 0.0),
            [1.0, 0.0, 0.0]
        ));
        let east = std::f32::consts::FRAC_PI_2;
        assert!(close(
            view_to_world([0.0, 1.0, 0.0], east, 0.0),
            [1.0, 0.0, 0.0]
        ));
        let down = std::f32::consts::FRAC_PI_2;
        assert!(close(
            view_to_world([0.0, 1.0, 0.0], 0.0, down),
            [0.0, 0.0, -1.0]
        ));
    }

    /// The force severs from `iDismemberForce`, a gun explodes an explodable limb from
    /// `iExplodeForce`, melee never explodes, and the roll's own outcome stands.
    #[test]
    fn heavy_hits_sever_and_the_heaviest_guns_explode() {
        let t = Tuning::PLAY;
        assert_eq!(
            outcome(&t, &gun(3, 20.0, 1, false), true, false, false),
            (false, false)
        );
        assert_eq!(
            outcome(&t, &gun(3, 48.0, 1, false), true, false, false),
            (true, false)
        );
        assert_eq!(
            outcome(&t, &gun(5, 110.0, 1, false), true, false, false),
            (false, true)
        );
        assert_eq!(
            outcome(&t, &gun(5, 110.0, 1, false), false, false, false),
            (true, false)
        );
        // A super sledge's weight severs but never explodes.
        assert_eq!(
            outcome(&t, &melee(55.0, 20.0), true, false, false),
            (true, false)
        );
        assert_eq!(
            outcome(&t, &melee(10.0, 1.0), true, false, false),
            (false, false)
        );
        assert_eq!(
            outcome(&t, &gun(3, 1.0, 1, false), true, false, true),
            (false, true)
        );
        let off = Tuning {
            dismember_force: 0,
            explode_force: 0,
            ..Tuning::PLAY
        };
        assert_eq!(
            outcome(&off, &gun(5, 500.0, 1, false), true, false, false),
            (false, false)
        );
    }

    #[test]
    fn heavier_melee_pushes_harder() {
        assert_eq!(impulse_scale(None), 1.0);
        assert_eq!(impulse_scale(Some(&gun(5, 50.0, 1, false))), 1.0);
        let knife = impulse_scale(Some(&melee(10.0, 1.0)));
        let sledge = impulse_scale(Some(&melee(35.0, 12.0)));
        assert!(knife < 1.0 && sledge > 1.5, "{knife} {sledge}");
    }

    /// The punch lasts 60 to 120 ms at the default, grows with weight and calibre, skips light
    /// shots, and is off with `bHitStop=0` or `iHitStopMs=0`.
    #[test]
    fn impacts_punch_by_weight_and_calibre() {
        let t = Tuning::PLAY;
        let fists = melee_punch(&t, None, 1.0).unwrap();
        let sledge = melee_punch(&t, Some(&melee(55.0, 20.0)), 1.0).unwrap();
        assert!(
            fists.seconds >= 0.06 && sledge.seconds <= 0.125,
            "{fists:?} {sledge:?}"
        );
        assert!(sledge.back > fists.back && sledge.lift > fists.lift);
        assert!(sledge.camera_pitch < fists.camera_pitch && fists.camera_pitch < 0.0);
        assert!(shot_punch(&t, &gun(3, 20.0, 1, false), false, 0.0).is_none());
        let shotgun = shot_punch(&t, &gun(5, 7.0, 8, false), false, 0.0).unwrap();
        let rifle = shot_punch(&t, &gun(5, 110.0, 1, false), false, 0.0).unwrap();
        assert!(rifle.back > shotgun.back && rifle.lift > shotgun.lift);
        assert!(rifle.seconds <= 0.125 && shotgun.seconds >= 0.06);
        let aimed = shot_punch(&t, &gun(5, 110.0, 1, false), true, 0.0).unwrap();
        assert!(aimed.back < rifle.back);
        assert!(shot_punch(&t, &melee(55.0, 20.0), false, 0.0).is_none());
        for off in [
            Tuning {
                hit_stop: false,
                ..Tuning::PLAY
            },
            Tuning {
                hit_stop_ms: 0,
                ..Tuning::PLAY
            },
        ] {
            assert!(melee_punch(&off, None, 1.0).is_none());
            assert!(shot_punch(&off, &gun(5, 110.0, 1, false), false, 1.0).is_none());
        }
    }

    /// The envelope peaks in the first fifth and is back at 0 by the end; the camera's turns over
    /// a punch add up to nothing, so the view ends where it started.
    #[test]
    fn a_punch_snaps_back_and_the_camera_ends_where_it_started() {
        let seconds = 0.09;
        assert_eq!(punch_envelope(0.0, seconds), 0.0);
        assert!((punch_envelope(0.2 * seconds - 1e-6, seconds) - 1.0).abs() < 1e-3);
        assert!(punch_envelope(0.8 * seconds, seconds) < 0.1);
        assert_eq!(punch_envelope(seconds, seconds), 0.0);
        let punch = melee_punch(&Tuning::PLAY, Some(&melee(35.0, 12.0)), -1.0).unwrap();
        let mut punches = vec![Active {
            punch,
            elapsed: 0.0,
            camera_share: 0.0,
        }];
        let (mut pitch, mut heading, mut peak) = (0.0f32, 0.0f32, 0.0f32);
        for _ in 0..30 {
            let (p, h, model) = punch_step(&mut punches, 1.0 / 144.0);
            pitch += p;
            heading += h;
            peak = peak.max(-model.offset[1]);
        }
        assert!(punches.is_empty());
        assert!(
            pitch.abs() < 1e-6 && heading.abs() < 1e-6,
            "{pitch} {heading}"
        );
        assert!(peak > 0.5 * punch.back, "{peak}");
    }

    /// No time dilation anywhere: the module never names the game's time multiplier.
    #[test]
    fn game_time_is_never_touched() {
        let source = include_str!("gunplay.rs");
        let forbidden = ["011a_c3a", "011ac3a", "fGlobalTimeMultiplier"];
        let code: String = source
            .lines()
            .filter(|l| !l.contains("forbidden"))
            .collect::<Vec<_>>()
            .join("\n");
        for word in forbidden {
            assert!(!code.contains(word), "{word}");
        }
    }

    #[test]
    fn rows_map_values_to_settings_and_back() {
        assert_eq!(Row::Recoil.setting_of(100), 10);
        assert_eq!(Row::Recoil.value_of(10), 100);
        assert_eq!(Row::Recoil.setting_of(1000), 20);
        assert_eq!(Row::Dismember.setting_of(45), 9);
        assert_eq!(Row::Dismember.value_of(30), 150);
        assert_eq!(Row::HitStop.setting_of(1), 1);
        assert_eq!(Row::Dismember.label(0), "Dismember Force: Off");
        assert_eq!(Row::Sway.label(80), "Weapon Sway: 80%");
        for row in ROWS {
            assert!(row.label(300).len() < LABEL_SIZE as usize);
            assert_eq!(
                row.value_of(row.setting_of(row.default_value())),
                row.default_value()
            );
        }
        assert_eq!(Row::Enabled.key(), ("nv-rs Fixes", "bWeightyCombat"));
    }
}
