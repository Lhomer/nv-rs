//! Weighty combat (`NV_WEIGHTY_COMBAT`), a row of the `fixes` preset ([`crate::fixes`],
//! docs/FIXES.md, plan in docs/GUNPLAY.md): **not 1:1 with FalloutNV.exe**, an nv-rs addition the
//! player asked for (Patrick, 2026-11-13: guns and melee that feel weighty, with recoil, kick,
//! inertia, hit-stop and reach, and hits that push bodies by calibre or weapon weight and can
//! sever limbs). In the `vanilla` preset, or with `bWeightyCombat=0`, nothing here runs.
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
//!   down), the hit-stop ends on time, and the weapon sway is placed ([`Sway`]).
//! - The reach search (`009a60e0`, the actor and its reach): the player's melee reach is scaled.
//! - `Actor::CombatHit` (`00899cb0`): the player's melee hit stops time for a moment, longer for a
//!   heavier weapon.
//! - The kill impulse (`008ae000`, the aggressor and the hit data): the strength the game pushes
//!   the killed body with (the hit's health damage at `+0x14`, times the weapon's kill impulse,
//!   at the hit's point) is scaled, by the weapon's weight for melee; a player's heavy kill stops
//!   time for a moment too.
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
use engine::units::fallout_shared::combatformulas::fn_00646e50;
use engine::units::fallout_shared::tesobjectweap::fn_00523150;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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
        REACH_SEARCH,
        COMBAT_HIT,
        KILL_IMPULSE,
        DISMEMBER_ROLL,
        CREATE_MENU_ELEMENTS,
    ],
    summary: "guns kick the view and recover, weapons sway, melee hits stop time for a moment and reach a little further, kills push bodies by calibre and weapon weight, and a heavy enough hit severs the limb (Settings menu rows and [nv-rs Gunplay] tune it)",
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
    /// `bHitStop`: melee and heavy hits stop time for a moment.
    pub hit_stop: bool,
    /// `iHitStopMs`: how long, in milliseconds of real time, for a melee hit of a light weapon.
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
        hit_stop_ms: 70,
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
/// The player's first-person 3D (`sp1stPerson3D`, an `NiPointer`), and an `NiAVObject`'s local
/// translation (`m_kLocal.m_Translate`).
const FIRST_PERSON_3D: u32 = 0x694;
const LOCAL_TRANSLATE: u32 = 0x34 + 0x24;
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
/// `BSTimer::fGlobalTimeMultiplier` and its target.
const TIME_MULTIPLIER: u32 = 0x011a_c3a0;
const TIME_MULTIPLIER_TARGET: u32 = 0x011a_c3a4;
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

/// The hit-stop of a melee hit with the weapon, in milliseconds: `iHitStopMs` for a light weapon,
/// up to 1.8 times it for the heaviest.
pub fn melee_hit_stop_ms(tuning: &Tuning, stats: Option<&WeaponStats>) -> u64 {
    let weight = stats.map_or(0.0, |s| s.weight);
    let scale = (0.8 + weight / 20.0).clamp(0.8, 1.8);
    (tuning.hit_stop_ms.max(0) as f32 * scale) as u64
}

// --- The state of play (the player's frame, one at a time). --------------------------------

/// The kick to turn in, the share to give back, the hit-stop and the sway.
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
    /// The time multiplier the hit-stop set and when it ends.
    hit_stop: Option<(Instant, f32)>,
    sway: Sway,
    /// A small random state for the sideways kick.
    seed: u32,
}

/// The weapon's offset from where the game puts it, in the view's axes (right, forward, up) in
/// game units, and what was last written to the first-person 3D.
#[derive(Debug, Default)]
pub struct Sway {
    offset: [f32; 3],
    /// The push back of a shot, decaying.
    push: f32,
    /// The walking bob's phase.
    phase: f32,
    /// The node it was written to, the translation written and the world offset in it.
    node: u32,
    written: [f32; 3],
    applied: [f32; 3],
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
        if on() && a[0] != 0 && a[1] != 0 && a[0] == e.global::<u32>(PLAYER) {
            let weapon = held_weapon(e, a[0]);
            let stats = weapon_stats(e, weapon);
            if stats.map_or(true, |s| s.is_melee()) {
                let ms = melee_hit_stop_ms(&tuning(), stats.as_ref());
                hit_stop(e, ms);
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

/// A shot of the player's: the kick to turn in and the weapon's push.
fn shot(e: &mut Engine, weapon: u32, player: u32) {
    if e.mem.i32(VATS_MODE) != 0 {
        return;
    }
    let Some(stats) = weapon_stats(e, weapon) else {
        return;
    };
    let sights = e.call(ACTOR_GET_IRON_SIGHTS, &args![player]).bool();
    let degrees = kick_degrees(&stats, sights) * tuning().recoil as f32 / 100.0;
    if degrees <= 0.0 {
        return;
    }
    with_state(|s| {
        let kick = degrees.to_radians();
        s.pending_pitch = (s.pending_pitch + kick).min(0.35);
        s.pending_heading += kick * 0.3 * wobble(&mut s.seed);
        s.sway.push = (s.sway.push + degrees * 0.35).min(3.0);
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
    let heavy = stats
        .is_some_and(|s| tuning.dismember_force > 0 && force(&s) >= tuning.dismember_force as f32);
    if heavy && aggressor != 0 && aggressor == e.global::<u32>(PLAYER) {
        hit_stop(e, (tuning.hit_stop_ms.max(0) as u64) * 3 / 4);
    }
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

/// Stops time for `ms` milliseconds of real time, unless the hit-stop is off or another slow
/// motion (VATS, a script's) is running.
fn hit_stop(e: &mut Engine, ms: u64) {
    if !tuning().hit_stop || ms == 0 || e.mem.i32(VATS_MODE) != 0 {
        return;
    }
    let multiplier = e.mem.f32(TIME_MULTIPLIER);
    let target = e.mem.f32(TIME_MULTIPLIER_TARGET);
    if multiplier != 1.0 || target != 1.0 {
        return;
    }
    const SLOW: f32 = 0.08;
    e.mem.set_f32(TIME_MULTIPLIER, SLOW);
    let until = Instant::now() + Duration::from_millis(ms.min(400));
    with_state(|s| s.hit_stop = Some((until, SLOW)));
}

/// The end of the hit-stop: time back to 1 when it is still the hit-stop's (the game's ramp,
/// `00aa4e40`, brings it back as well if this frame never comes).
fn end_hit_stop(e: &mut Engine, s: &mut State, now: Instant) {
    let Some((until, slow)) = s.hit_stop else {
        return;
    };
    if now < until {
        return;
    }
    s.hit_stop = None;
    if e.mem.f32(TIME_MULTIPLIER) == slow && e.mem.f32(TIME_MULTIPLIER_TARGET) == 1.0 {
        e.mem.set_f32(TIME_MULTIPLIER, 1.0);
    }
}

/// Once a frame of the player's look handling: the hit-stop's end, the kick and its recovery, the
/// sway.
fn frame(e: &mut Engine, player: u32, time_step: f32) {
    let now = Instant::now();
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(|| State {
        seed: 0x9e37_79b9,
        ..State::default()
    });
    end_hit_stop(e, s, now);
    let dt = if time_step.is_finite() {
        time_step.clamp(0.0, 0.25)
    } else {
        0.0
    };
    if !on() || e.mem.i32(VATS_MODE) != 0 || dt <= 0.0 {
        remove_sway(e, &mut s.sway);
        s.pending_pitch = 0.0;
        s.pending_heading = 0.0;
        s.debt = 0.0;
        s.last_pitch = None;
        s.last_heading = None;
        s.last_position = None;
        return;
    }
    let tuning = tuning();
    // The player's own turn since the last frame (before this frame's kick).
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
    if up != 0.0 || back != 0.0 {
        e.call(ADD_LOOKING, &args![player, back - up]);
    }
    if side != 0.0 {
        e.call(ADD_HEADING, &args![player, side]);
    }
    let pitch_after = e.call(GET_LOOKING, &args![player]).f64() as f32;
    let heading_after = e.vcall(player, HEADING_SLOT, &args![0u32]).f64() as f32;
    s.last_pitch = Some(pitch_after);
    s.last_heading = Some(heading_after);
    // The sway, in first person only.
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
    if !shown || tuning.sway == 0 {
        remove_sway(e, &mut s.sway);
        s.sway = Sway::default();
        return;
    }
    let sights = e.call(ACTOR_GET_IRON_SIGHTS, &args![player]).bool();
    let strength = tuning.sway as f32 / 100.0 * if sights { 0.2 } else { 1.0 };
    let recoil = tuning.recoil as f32 / 100.0;
    let offset = sway_step(
        &mut s.sway,
        turned_heading / dt,
        turned_pitch / dt,
        moved,
        strength,
        recoil,
        dt,
    );
    let node = e.mem.u32(player + FIRST_PERSON_3D);
    let world = view_to_world(offset, heading_after, pitch_after);
    place_sway(e, &mut s.sway, node, world);
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

/// One frame of the sway: the weapon lags the turn (radians a second), bobs with the walk (units
/// a second) and is pushed back by a shot, smoothed by a spring; the offset in the view's axes.
pub fn sway_step(
    sway: &mut Sway,
    heading_rate: f32,
    pitch_rate: f32,
    speed: f32,
    strength: f32,
    recoil: f32,
    dt: f32,
) -> [f32; 3] {
    let lag = 0.35 * strength;
    let speed = sane(speed, 0.0, 600.0);
    sway.phase = (sway.phase + dt * speed * 0.03) % std::f32::consts::TAU;
    let bob = (speed / 300.0).min(1.0) * 0.3 * strength;
    let target = [
        (-heading_rate * lag).clamp(-1.2, 1.2) + bob * sway.phase.sin(),
        -sway.push * recoil,
        (pitch_rate * lag).clamp(-1.0, 1.0) + bob * 0.5 * (2.0 * sway.phase).sin().abs()
            - bob * 0.25,
    ];
    let follow = 1.0 - (-dt / 0.09).exp();
    for (now, want) in sway.offset.iter_mut().zip(target) {
        *now += (want - *now) * follow;
    }
    sway.push *= (-dt / 0.08).exp();
    sway.offset
}

/// The view's (right, forward, up) offset in the world for `heading` (around z, 0 north,
/// clockwise) and `pitch` (positive down).
pub fn view_to_world(offset: [f32; 3], heading: f32, pitch: f32) -> [f32; 3] {
    let (sh, ch) = heading.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let right = [ch, -sh, 0.0];
    let forward = [sh * cp, ch * cp, -sp];
    let up = [sh * sp, ch * sp, cp];
    let mut world = [0.0; 3];
    for (i, w) in world.iter_mut().enumerate() {
        *w = right[i] * offset[0] + forward[i] * offset[1] + up[i] * offset[2];
    }
    world
}

/// The first-person 3D's translation with the sway in it: from where the game put it when the game
/// wrote it since, else from where it was before the last sway.
fn place_sway(e: &mut Engine, sway: &mut Sway, node: u32, world: [f32; 3]) {
    if node == 0 {
        sway.node = 0;
        return;
    }
    let at = node + LOCAL_TRANSLATE;
    let now = [e.mem.f32(at), e.mem.f32(at + 4), e.mem.f32(at + 8)];
    let mut base = now;
    if sway.node == node && now == sway.written {
        for (b, a) in base.iter_mut().zip(sway.applied) {
            *b -= a;
        }
    }
    let mut written = base;
    for (w, o) in written.iter_mut().zip(world) {
        *w += o;
    }
    for (i, value) in written.iter().enumerate() {
        e.mem.set_f32(at + 4 * i as u32, *value);
    }
    sway.node = node;
    sway.written = written;
    sway.applied = world;
}

/// The sway taken out again, when the translation is still the one written.
fn remove_sway(e: &mut Engine, sway: &mut Sway) {
    if sway.node == 0 {
        return;
    }
    let at = sway.node + LOCAL_TRANSLATE;
    let now = [e.mem.f32(at), e.mem.f32(at + 4), e.mem.f32(at + 8)];
    if now == sway.written {
        for (i, (value, applied)) in now.iter().zip(sway.applied).enumerate() {
            e.mem.set_f32(at + 4 * i as u32, value - applied);
        }
    }
    sway.node = 0;
    sway.offset = [0.0; 3];
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
    /// Off / On: `bHitStop`.
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
            Row::HitStop => "Hit-Stop".to_string(),
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

    #[test]
    fn the_sway_lags_the_turn_and_settles() {
        let mut sway = Sway::default();
        let offset = sway_step(&mut sway, 2.0, 0.0, 0.0, 1.0, 1.0, 0.1);
        assert!(
            offset[0] < 0.0,
            "a turn to the right leaves the weapon to the left: {offset:?}"
        );
        for _ in 0..200 {
            sway_step(&mut sway, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0 / 60.0);
        }
        assert!(
            sway.offset.iter().all(|v| v.abs() < 1e-3),
            "{:?}",
            sway.offset
        );
        // A shot pushes it back towards the player, then it returns.
        sway.push = 1.0;
        let offset = sway_step(&mut sway, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0 / 60.0);
        assert!(offset[1] < 0.0);
        // Nothing moves with no strength and no recoil.
        let mut still = Sway {
            push: 1.0,
            ..Sway::default()
        };
        assert_eq!(
            sway_step(&mut still, 5.0, 5.0, 300.0, 0.0, 0.0, 0.1),
            [0.0; 3]
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
    fn heavier_melee_pushes_harder_and_stops_longer() {
        assert_eq!(impulse_scale(None), 1.0);
        assert_eq!(impulse_scale(Some(&gun(5, 50.0, 1, false))), 1.0);
        let knife = impulse_scale(Some(&melee(10.0, 1.0)));
        let sledge = impulse_scale(Some(&melee(35.0, 12.0)));
        assert!(knife < 1.0 && sledge > 1.5, "{knife} {sledge}");
        let t = Tuning::PLAY;
        assert_eq!(melee_hit_stop_ms(&t, None), 56);
        assert!(melee_hit_stop_ms(&t, Some(&melee(55.0, 20.0))) > 100);
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
