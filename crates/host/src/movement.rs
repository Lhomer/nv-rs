//! Snappy movement (Patrick, 2026-11-14: movement that feels like Counter-Strike's): the player's
//! walking, running and jumping driven by the Quake player-movement model. Part of the weighty
//! combat row ([`crate::gunplay::FIX`]): `bWeightyCombat=0` or `sPreset=vanilla` turn it off with
//! the rest, `[nv-rs Movement] bSnappyMovement=0` alone. **Not 1:1 with FalloutNV.exe.** Only the
//! player's character changes; every other actor moves as in the original.
//!
//! **Credit.** The model is id Software's, from the Quake III Arena source (`code/game/bg_pmove.c`:
//! `PM_Friction`, `PM_Accelerate`, `PM_AirMove`; Copyright (C) 1999-2005 Id Software, Inc.,
//! released under the GNU General Public License version 2 or, at your option, any later version,
//! which nv-rs's GPLv3 takes in; distribution/THIRD-PARTY-NOTICES.txt). The friction and the
//! acceleration below are those functions' arithmetic in Rust; the air step caps the wished speed
//! as Quake (1996, `SV_AirAccelerate`, the same licence) does, which gives the small air-strafe
//! Counter-Strike players know. Valve's Source SDK was not used.
//!
//! **What the original does.** The player's speed comes from the movement animation's root
//! motion (`PlayerMover::Update`, `009e9e50`, via `00494390`), scaled to the formula speed
//! (`Actor::CalculateRunSpeed`, `00885bf0`): starting, stopping and turning follow the animation's
//! blend, a few tenths of a second each. The character controller's ground state
//! (`bhkCharacterStateOnGround::Update`, `00cd4800`) then takes that velocity at once through
//! Havok's movement util (`hkpCharacterMovementUtil::calculateMovement`, `00d6aef0`, gain 1); the
//! air state (`00cd3fb0`) steers toward it with `fAcrobatics`.
//!
//! **What else the original lets decide** (Patrick, ebf57acb: jumping left slowed him in the air,
//! a slowdown step after landing, no quick change of direction). The frame's move vector, the
//! animation's root motion as `PlayerMover::Update` rescales and turns it, goes through
//! `MobileObject::Move` (`0092f260`) into `bhkCharacterController::Move`'s parameters (`+0x10`),
//! and three things follow from it there: the controller's `Direction` (the movement util's
//! wanted velocity: the animation's direction, and each direction's own animation speed, a
//! strafe's slower), and the stillness rule: a move vector within 1/20 on each lane with no
//! velocity modifier makes the controller "still", and a still controller whose contacts have not
//! changed gets its velocity zeroed and is not integrated (`move_on_ground`), which is every frame
//! the animation gives no root motion: the first frames of a press, a landing animation, a jump
//! animation's strafe. The jump state takes off with the proxy's velocity, which that rule may
//! have zeroed. So in Snappy mode the move vector is the keys' ([`keys_move`], over
//! `MobileObject::Move`): their direction (`SideVec` for a stick, else the `MoveMode` bits) at the
//! formula speed for the frame's seconds; a sliding player with no key gets a nudge so friction,
//! not the rule, stops it. The settings `fJumpMoveMult` and `fJumpMoveBase` have no reader (the
//! air state's gain is its own statics), and the game has no strafe speed setting: strafing was
//! slower only by its animation.
//!
//! **The debug log** (`bMovementLog=1`): for each of the player's steps, the move vector replaced,
//! the Snappy step (state, keys, wish, velocity before and after), and after `Move` the final
//! velocity with the path that set it (snappy; original; zeroed after snappy; changed by the
//! game); in Classic mode the util's own result.
//!
//! **What this does.** Over the movement util, for the player's controller only (its ground and
//! air states, the util's two callers that move the player by keys), after the util ran:
//! - the wish: the direction the game wants (the util's own frame, the animation's direction, or
//!   the keys' (`PlayerMover::SideVec`) when the animation gives none yet) at the formula speed,
//!   run or walk as the keys say (`PlayerMover::MoveMode` 0x200 / 0x100), sneaking included; no
//!   movement key (`MoveMode` & 0xF) is a wish of nothing, so the player stops at once instead of
//!   gliding out of the animation;
//! - on the ground, Quake's friction (`iFriction`, with `iStopSpeed` below which it stops as from
//!   that speed: a sharp stop) and then its acceleration toward the wish (`iAccelerate`): full
//!   speed in about a tenth of a second, a reversal as quick;
//! - in the air, the acceleration with the wished speed capped at `iAirSpeedCap` (`iAirAccelerate`):
//!   a little air-strafe, no friction;
//! - the velocity across the surface (the ground state) or across the up axis (the air state) is
//!   replaced; the game keeps the rest: the vertical part, gravity, a push (`ApplyVelocityMod`),
//!   the collision.
//!
//! **Jump and landing.** Over the air state, gravity is `iJumpGravity` percent of the game's while
//! the player is in the air, and over the jump state (`bhkCharacterStateJumping::Update`,
//! `00cd4280`) the take-off speed is the square root of that times the game's, so a jump is as
//! high as the original's and over sooner: less floaty, a quicker landing.
//!
//! **The Settings menu** (Patrick: every value editable in game, kept across launches): ten rows
//! on the Gameplay page ([`ROWS`]): Movement (Classic / Snappy), Ground Acceleration, Ground
//! Friction, Stop Speed, Air Control, Air Speed Cap, Run Speed (`iRunSpeed`, the wished speed's
//! percent), Jump Strength (`iJumpStrength`, the height's percent), Jump Gravity and Reset Movement
//! Settings. A change applies at the next physics step and is saved to Fallout.ini's
//! `[nv-rs Movement]` in a launcher's play run, as the weighty combat rows do; the page's own
//! Defaults puts each row back too.
//!
//! **The game moves the player itself** (a script, an AI package with the controls off): no key
//! has been down for [`KEYS_IDLE`] s while the animation still asks for movement; then the
//! original's movement runs as it is.
//!
//! **The camera.** The first-person camera stands at the player's eye every frame; the original
//! smooths nothing of it on movement, and nothing here adds smoothing. The animations play as the
//! game picks them; their speed follows the formula speed the wish uses, so a steady walk or run
//! matches them as before.

use engine::prelude::*;
use engine::units::bshavok::bhkcharacterstateinair::bhk_character_state_in_air_update;
use engine::units::bshavok::bhkcharacterstatejumping::bhk_character_state_jumping_update;
use engine::units::fallout_ai::mobileobject::mobile_object_move;
use engine::units::havok_sdk::hkpcharactermovementutil::hkp_character_movement_util_calculate_movement;
use std::sync::Mutex;

/// The Fallout.ini section of the tuning.
pub const SECTION: &str = "nv-rs Movement";

/// `hkpCharacterMovementUtil::calculateMovement`, the air state's and the jump state's updates.
pub const CALCULATE_MOVEMENT: u32 = 0x00d6_aef0;
pub const IN_AIR: u32 = 0x00cd_3fb0;
pub const JUMPING: u32 = 0x00cd_4280;

/// The tuning (module documentation). Speeds are in game units a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tuning {
    /// `bSnappyMovement`.
    pub on: bool,
    /// `iAccelerate`: Quake's `pm_accelerate` (10).
    pub accelerate: i32,
    /// `iFriction`: Quake's `pm_friction` (6).
    pub friction: i32,
    /// `iStopSpeed`: Quake's `pm_stopspeed` (100), in game units a second.
    pub stop_speed: i32,
    /// `iAirAccelerate`: the air acceleration (10).
    pub air_accelerate: i32,
    /// `iAirSpeedCap`: the wished speed the air step accelerates up to (30 units a second).
    pub air_speed_cap: i32,
    /// `iJumpGravity`: gravity in the air, in percent of the game's (130).
    pub jump_gravity: i32,
    /// `iRunSpeed`: the wished speed on the ground (running and walking), in percent of the
    /// game's formula speed (100).
    pub run_speed: i32,
    /// `iJumpStrength`: a jump's height, in percent of the game's (100).
    pub jump_strength: i32,
    /// `bMovementLog`: a log line for each of the player's movement steps, saying which path set
    /// the final velocity (off).
    pub log: bool,
}

impl Tuning {
    /// The built-in feel: the values of `crates/host/data/game_ini.tsv` (a test holds them).
    pub const PLAY: Tuning = Tuning {
        on: true,
        accelerate: 10,
        friction: 6,
        stop_speed: 100,
        air_accelerate: 10,
        air_speed_cap: 30,
        jump_gravity: 130,
        run_speed: 100,
        jump_strength: 100,
        log: false,
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
            "bsnappymovement" => self.on = value != 0,
            "iaccelerate" => self.accelerate = value,
            "ifriction" => self.friction = value,
            "istopspeed" => self.stop_speed = value,
            "iairaccelerate" => self.air_accelerate = value,
            "iairspeedcap" => self.air_speed_cap = value,
            "ijumpgravity" => self.jump_gravity = value,
            "irunspeed" => self.run_speed = value,
            "ijumpstrength" => self.jump_strength = value,
            "bmovementlog" => self.log = value != 0,
            _ => return false,
        }
        true
    }

    /// The value of `key` (a [`KEYS`] key).
    pub fn get(&self, key: &str) -> Option<i32> {
        Some(match key.to_ascii_lowercase().as_str() {
            "bsnappymovement" => i32::from(self.on),
            "iaccelerate" => self.accelerate,
            "ifriction" => self.friction,
            "istopspeed" => self.stop_speed,
            "iairaccelerate" => self.air_accelerate,
            "iairspeedcap" => self.air_speed_cap,
            "ijumpgravity" => self.jump_gravity,
            "irunspeed" => self.run_speed,
            "ijumpstrength" => self.jump_strength,
            "bmovementlog" => i32::from(self.log),
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
pub const KEYS: [(&str, &str, i32, i32); 10] = [
    ("bSnappyMovement", "NV_MOVEMENT_SNAPPY", 0, 1),
    ("iAccelerate", "NV_MOVEMENT_ACCELERATE", 1, 50),
    ("iFriction", "NV_MOVEMENT_FRICTION", 0, 20),
    ("iStopSpeed", "NV_MOVEMENT_STOP_SPEED", 0, 500),
    ("iAirAccelerate", "NV_MOVEMENT_AIR_ACCELERATE", 0, 100),
    ("iAirSpeedCap", "NV_MOVEMENT_AIR_SPEED_CAP", 0, 300),
    ("iJumpGravity", "NV_MOVEMENT_JUMP_GRAVITY", 50, 300),
    ("iRunSpeed", "NV_MOVEMENT_RUN_SPEED", 50, 200),
    ("iJumpStrength", "NV_MOVEMENT_JUMP_STRENGTH", 50, 200),
    ("bMovementLog", "NV_MOVEMENT_LOG", 0, 1),
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

static TUNING: Mutex<Tuning> = Mutex::new(Tuning::PLAY);

/// The tuning the run starts with (`crate::fixes::install`, before the hooks).
pub fn set_tuning(tuning: Tuning) {
    *TUNING.lock().unwrap_or_else(|p| p.into_inner()) = tuning;
}

/// The tuning in force.
pub fn tuning() -> Tuning {
    *TUNING.lock().unwrap_or_else(|p| p.into_inner())
}

// --- Quake's arithmetic (credit: module documentation). ----------------------------------------

/// `PM_Friction`: the speed taken off on the ground in `dt` seconds, from at least `stop_speed`
/// (a slow slide stops as quickly as from that speed).
pub fn friction(v: [f32; 2], dt: f32, friction: f32, stop_speed: f32) -> [f32; 2] {
    let speed = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if speed < 1e-3 {
        return [0.0, 0.0];
    }
    let control = speed.max(stop_speed);
    let new_speed = (speed - control * friction * dt).max(0.0);
    let k = new_speed / speed;
    [v[0] * k, v[1] * k]
}

/// `PM_Accelerate`: toward `wish_speed` along the unit `wish_dir`, at most `accelerate * dt *
/// wish_speed` in `dt` seconds, never past the wished speed along it (sideways speed is kept,
/// friction takes it).
pub fn accelerate(
    v: [f32; 2],
    wish_dir: [f32; 2],
    wish_speed: f32,
    accelerate: f32,
    dt: f32,
) -> [f32; 2] {
    let current = v[0] * wish_dir[0] + v[1] * wish_dir[1];
    let add = wish_speed - current;
    if add <= 0.0 {
        return v;
    }
    let step = (accelerate * dt * wish_speed).min(add);
    [v[0] + step * wish_dir[0], v[1] + step * wish_dir[1]]
}

/// The air step: [`accelerate`] with the speed along the wish capped at `cap`, the step from
/// the full wished speed (Quake's `SV_AirAccelerate`): turning in the air bends the path a little.
pub fn air_accelerate(
    v: [f32; 2],
    wish_dir: [f32; 2],
    wish_speed: f32,
    cap: f32,
    accelerate: f32,
    dt: f32,
) -> [f32; 2] {
    let current = v[0] * wish_dir[0] + v[1] * wish_dir[1];
    let add = wish_speed.min(cap) - current;
    if add <= 0.0 {
        return v;
    }
    let step = (accelerate * dt * wish_speed).min(add);
    [v[0] + step * wish_dir[0], v[1] + step * wish_dir[1]]
}

// --- The game's frame. -------------------------------------------------------------------------

type V4 = [f32; 4];

fn cross(a: V4, b: V4) -> V4 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
        0.0,
    ]
}

fn dot(a: V4, b: V4) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: V4) -> Option<V4> {
    let length = dot(a, a).sqrt();
    (length > 1e-6).then(|| [a[0] / length, a[1] / length, a[2] / length, 0.0])
}

/// The movement util's frame (`00d6aef0`): from the forward, up and surface vectors, the two
/// axes across the surface (the wish's first and second local parts) and the surface's normal.
pub fn frame(forward: V4, up: V4, surface: V4) -> Option<[V4; 3]> {
    let a = unit(cross(forward, up))?;
    let b = unit(cross(a, surface))?;
    let c = unit(cross(b, surface))?;
    Some([b, c, surface])
}

// --- The hooks. --------------------------------------------------------------------------------

/// Seconds without a movement key after which the game is taken to move the player itself.
pub const KEYS_IDLE: f32 = 0.6;
/// A wish slower than this (Havok units a second) is no direction.
const NO_WISH: f32 = 0.05;

/// The player's character's controller states (`hkpCharacterContext::m_currentState`).
const STATE_ON_GROUND: u32 = 0;
const STATE_IN_AIR: u32 = 2;
/// Controller fields: the current state, the step's seconds, `OutVelocity`, the wanted state and
/// `fGravity`.
const CURRENT_STATE: u32 = 0x3f0;
const STEP_SECONDS: u32 = 0x4e8;
const OUT_VELOCITY: u32 = 0x4f0;
const WANT_STATE: u32 = 0x520;
const GRAVITY: u32 = 0x54c;
/// The player, `MobileObject::GetCharController`, the actor's mover (`Actor::pActorMover`) and the
/// mover's `SideVec` and `MoveMode`; the actor's walk and run speed slots (`CalculateWalkSpeed`,
/// `CalculateRunSpeed`); the Havok-to-game scale (`011c3d18`).
const PLAYER: u32 = 0x011d_ea3c;
const GET_CHAR_CONTROLLER: u32 = 0x0093_06d0;
const ACTOR_MOVER: u32 = 0x190;
const SIDE_VEC: u32 = 0x88;
const MOVE_MODE: u32 = 0x94;
const SLOT_WALK_SPEED: u32 = 0x458;
const SLOT_RUN_SPEED: u32 = 0x45c;
const HAVOK_TO_GAME: u32 = 0x011c_3d18;
/// `MoveMode`'s direction keys and its walk and run bits.
const KEYS_MASK: u32 = 0xf;
const WALK: u32 = 0x100;
const RUN: u32 = 0x200;

/// What the hooks keep between steps (the player's alone).
struct State {
    /// Seconds since a movement key was last down.
    idle: f32,
    /// The frame's move vector as the animation gave it was not still ([`MOBILE_MOVE`]).
    animation_moving: bool,
    /// The move vector was the keys' this frame (or a nudge to let friction finish a slide).
    injected: bool,
    /// The velocity [`snap`] wrote this frame (for the log: who set the final velocity).
    wrote: Option<[f32; 3]>,
    /// The player's speed across the ground after the last step, Havok units a second.
    speed: f32,
}

static STATE: Mutex<State> = Mutex::new(State {
    idle: 0.0,
    animation_moving: false,
    injected: false,
    wrote: None,
    speed: 0.0,
});

fn kept() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

/// `MobileObject::Move` (`0092f260`): the move vector it hands the controller.
pub const MOBILE_MOVE: u32 = 0x0092_f260;
/// `MoveMode`'s swimming and flying bits: the original's movement then.
const SWIMMING_OR_FLYING: u32 = 0x800 | 0x2000;
/// A move vector shorter than this per frame is "still" to the controller (`Move`'s
/// `POINTS_NEAR` with a tolerance of 1/20 on each lane): then it zeroes the velocity when the
/// contacts have not changed.
const STILL: f32 = 0.05;
/// The nudge that keeps a sliding player from counting as still, units a frame.
const NUDGE: f32 = 0.1;

/// The keys' direction in the mover's frame (x to the right, y forward), unit length: the mover's
/// `SideVec` when it has one (a stick), else the direction bits of `MoveMode` (0x1 forward, 0x2
/// back, 0x4 left, 0x8 right; left is `-x` as in `PlayerMover::Update`). `None` without one.
pub fn key_direction(mode: u32, side: [f32; 2]) -> Option<[f32; 2]> {
    if mode & KEYS_MASK == 0 {
        return None;
    }
    let length = (side[0] * side[0] + side[1] * side[1]).sqrt();
    if length > 1e-3 {
        return Some([side[0] / length, side[1] / length]);
    }
    let bit = |b: u32| if mode & b != 0 { 1.0f32 } else { 0.0 };
    let (x, y) = (bit(8) - bit(4), bit(1) - bit(2));
    let length = (x * x + y * y).sqrt();
    (length > 0.0).then(|| [x / length, y / length])
}

/// Whether the debug log is on (`bMovementLog`).
fn logging() -> bool {
    tuning().log
}

/// Registers the hooks (from `crate::gunplay::install`).
pub fn install(e: &mut Engine) {
    install_change_fns(e);
    e.register(MOBILE_MOVE, |e, a| {
        let this = a[0];
        let player = e.global::<u32>(PLAYER);
        let mine = player != 0 && this == player;
        if mine {
            keys_move(e, player, f32::from_bits(a[1]), a[2]);
        }
        let controller = mobile_object_move(
            e,
            Ptr::new(this),
            f32::from_bits(a[1]),
            Ptr::new(a[2]),
            a[3],
        );
        if mine {
            after_move(e, controller.addr());
        }
        controller.into_ret()
    });
    e.register(CALCULATE_MOVEMENT, |e, a| {
        hkp_character_movement_util_calculate_movement(e, Ptr::new(a[0]), Ptr::new(a[1]));
        if let Some(controller) = player_output(e, a[1]) {
            snap(e, controller, a[0], a[1]);
        } else if logging() {
            classic_line(e, a[1]);
        }
        Ret::default()
    });
    e.register(IN_AIR, |e, a| {
        bhk_character_state_in_air_update(e, Ptr::new(a[0]), Ptr::new(a[1]));
        let c = a[1];
        if e.mem.u32(c + WANT_STATE) == 0xb && is_player(e, c) {
            heavier_fall(e, c);
        }
        Ret::default()
    });
    e.register(JUMPING, |e, a| {
        let c = a[1];
        let wanted = e.mem.u32(c + WANT_STATE);
        bhk_character_state_jumping_update(e, Ptr::new(a[0]), Ptr::new(a[1]));
        let jumped = wanted == 0xb || (wanted != 0 && wanted != 3);
        if jumped && is_player(e, c) {
            let t = tuning();
            if t.on && crate::gunplay::is_on() {
                let k = jump_speed_factor(t.jump_gravity, t.jump_strength);
                let z = e.mem.f32(c + OUT_VELOCITY + 8);
                e.mem.set_f32(c + OUT_VELOCITY + 8, z * k);
            }
        }
        Ret::default()
    });
}

/// The take-off speed's factor for gravity at `gravity` percent and a jump `strength` percent as
/// high as the game's (a height is the speed squared over twice the gravity).
pub fn jump_speed_factor(gravity: i32, strength: i32) -> f32 {
    (gravity as f32 / 100.0 * strength as f32 / 100.0).sqrt()
}

/// Whether `controller` is the player's.
fn is_player(e: &mut Engine, controller: u32) -> bool {
    let player = e.global::<u32>(PLAYER);
    player != 0 && e.call(GET_CHAR_CONTROLLER, &args![player]).u32() == controller
}

/// The player's controller when `output` is its `OutVelocity` and snappy movement is on.
fn player_output(e: &mut Engine, output: u32) -> Option<u32> {
    let t = tuning();
    if !t.on || !crate::gunplay::is_on() {
        return None;
    }
    let controller = output.checked_sub(OUT_VELOCITY)?;
    is_player(e, controller).then_some(controller)
}

/// The air state's gravity made `iJumpGravity` percent of the game's: the rest of it added, as
/// the state adds its own (`gravity * vector * step`, the vector from the controller's virtual
/// `0x94`, `+0x10`).
fn heavier_fall(e: &mut Engine, c: u32) {
    let t = tuning();
    if !t.on || !crate::gunplay::is_on() || t.jump_gravity == 100 {
        return;
    }
    let extra = (t.jump_gravity - 100) as f32 / 100.0;
    let vector_at = e.vcall(c, 0x94, &args![]).u32();
    if vector_at == 0 {
        return;
    }
    let gravity = e.mem.f32(c + GRAVITY);
    let step = e.mem.f32(c + STEP_SECONDS);
    for i in 0..3 {
        let g = e.mem.f32(vector_at + 0x10 + 4 * i);
        let at = c + OUT_VELOCITY + 4 * i;
        let v = e.mem.f32(at);
        e.mem.set_f32(at, v + extra * gravity * g * step);
    }
}

fn read4(e: &Engine, at: u32) -> V4 {
    [0, 1, 2, 3].map(|i| e.mem.f32(at + 4 * i))
}

/// The player's velocity across the surface (ground) or the up axis (air), Quake's way, written
/// over what the movement util gave (module documentation). `input` is the util's record.
fn snap(e: &mut Engine, c: u32, input: u32, output: u32) {
    let state = e.mem.u32(c + CURRENT_STATE);
    let ground = state == STATE_ON_GROUND;
    if !ground && state != STATE_IN_AIR {
        return;
    }
    let dt = e.mem.f32(c + STEP_SECONDS);
    if !(dt > 0.0 && dt < 0.5) {
        return;
    }
    let forward = read4(e, input + 0x10);
    let up = read4(e, input + 0x20);
    let surface = read4(e, input + 0x30);
    let current = read4(e, input + 0x40);
    let desired = read4(e, input + 0x50);
    let origin = read4(e, input + 0x70);
    let Some([b, cc, n]) = frame(forward, up, surface) else {
        return;
    };
    let relative = [0, 1, 2, 3].map(|i| current[i] - origin[i]);
    let v = [dot(relative, b), dot(relative, cc)];
    let v_normal = dot(relative, n);
    let player = e.global::<u32>(PLAYER);
    let mover = if player == 0 {
        0
    } else {
        e.mem.u32(player + ACTOR_MOVER)
    };
    if mover == 0 {
        return;
    }
    let mode = e.mem.u32(mover + MOVE_MODE);
    let keys = mode & KEYS_MASK != 0;
    let game_wish = [desired[0], desired[1]];
    let game_speed = (game_wish[0] * game_wish[0] + game_wish[1] * game_wish[1]).sqrt();
    let (idle, animation_moving, injected) = {
        let mut s = kept();
        s.idle = if keys { 0.0 } else { s.idle + dt };
        (s.idle, s.animation_moving, s.injected)
    };
    if !keys && idle > KEYS_IDLE && animation_moving && !injected {
        // The game moves the player itself: its own movement stands.
        if logging() {
            eprintln!("Movement: original (no key for {idle:.1} s, the game moves the player)");
        }
        return;
    }
    let scale = {
        let s = e.mem.f32(HAVOK_TO_GAME);
        if s.is_finite() && s > 0.1 {
            s
        } else {
            7.0
        }
    };
    let (wish_dir, wish_speed) = if keys {
        let slot = if mode & RUN == 0 && mode & WALK != 0 {
            SLOT_WALK_SPEED
        } else {
            SLOT_RUN_SPEED
        };
        let speed =
            e.vcall(player, slot, &args![]).f32() / scale * tuning().run_speed as f32 / 100.0;
        let dir = if game_speed > NO_WISH {
            [game_wish[0] / game_speed, game_wish[1] / game_speed]
        } else {
            // The keys' direction (x sideways, y forward) in the util's order (forward, sideways).
            let side = read4(e, mover + SIDE_VEC);
            let length = (side[0] * side[0] + side[1] * side[1]).sqrt();
            if length > 1e-6 {
                [side[1] / length, side[0] / length]
            } else {
                [0.0, 0.0]
            }
        };
        (
            dir,
            if speed.is_finite() {
                speed.max(0.0)
            } else {
                0.0
            },
        )
    } else {
        ([0.0, 0.0], 0.0)
    };
    let t = tuning();
    let moved = if ground {
        let slowed = friction(v, dt, t.friction as f32, t.stop_speed as f32 / scale);
        accelerate(slowed, wish_dir, wish_speed, t.accelerate as f32, dt)
    } else {
        air_accelerate(
            v,
            wish_dir,
            wish_speed,
            t.air_speed_cap as f32 / scale,
            t.air_accelerate as f32,
            dt,
        )
    };
    // The surface's part: the util's own on the ground (the game keeps its vertical speed after),
    // the current one in the air (the air state gives back its up part after).
    let normal = if ground { desired[2] } else { v_normal };
    let out = [0, 1, 2].map(|i| origin[i] + b[i] * moved[0] + cc[i] * moved[1] + n[i] * normal);
    let before = [0, 1, 2].map(|i| e.mem.f32(output + 4 * i as u32));
    for (i, value) in out.into_iter().enumerate() {
        e.mem.set_f32(output + 4 * i as u32, value);
    }
    {
        let mut s = kept();
        s.wrote = Some(out);
        s.speed = (moved[0] * moved[0] + moved[1] * moved[1]).sqrt();
    }
    if logging() {
        eprintln!(
            "Movement: snappy {} dt {dt:.4} keys {mode:#x} wish ({:.2}, {:.2}) x {wish_speed:.2} v ({:.2}, {:.2}) -> ({:.2}, {:.2}); the util gave ({:.2}, {:.2}, {:.2}), now ({:.2}, {:.2}, {:.2})",
            if ground { "ground" } else { "air" },
            wish_dir[0],
            wish_dir[1],
            v[0],
            v[1],
            moved[0],
            moved[1],
            before[0],
            before[1],
            before[2],
            out[0],
            out[1],
            out[2],
        );
    }
}

/// The frame's move vector, before `MobileObject::Move` hands it to the controller (Snappy only):
/// with a direction key down, the keys' direction at the formula speed (run or walk, `iRunSpeed`)
/// for the frame's seconds, so neither the animation's root motion nor its strafe speeds decide
/// the direction or the speed, and the controller never counts the player as still while a key is
/// down (`Move` zeroes a still controller's velocity: the step after landing, the first frames of
/// a press). Without a key, a player still sliding gets a nudge so friction, not the stillness
/// rule, stops it.
fn keys_move(e: &mut Engine, player: u32, seconds: f32, input: u32) {
    let original = read4(e, input);
    let moving =
        original[0].abs() > STILL || original[1].abs() > STILL || original[2].abs() > STILL;
    {
        let mut s = kept();
        s.animation_moving = moving;
        s.injected = false;
        s.wrote = None;
    }
    let t = tuning();
    if !t.on || !crate::gunplay::is_on() || input == 0 || !(seconds > 0.0 && seconds < 0.5) {
        return;
    }
    let mover = e.mem.u32(player + ACTOR_MOVER);
    if mover == 0 {
        return;
    }
    let mode = e.mem.u32(mover + MOVE_MODE);
    if mode & SWIMMING_OR_FLYING != 0 {
        return;
    }
    let side = read4(e, mover + SIDE_VEC);
    let replaced = match key_direction(mode, [side[0], side[1]]) {
        Some(dir) => {
            let slot = if mode & RUN == 0 && mode & WALK != 0 {
                SLOT_WALK_SPEED
            } else {
                SLOT_RUN_SPEED
            };
            let speed = e.vcall(player, slot, &args![]).f32() * t.run_speed as f32 / 100.0;
            if !speed.is_finite() || speed <= 0.0 {
                return;
            }
            let step = speed * seconds;
            [dir[0] * step, dir[1] * step, original[2]]
        }
        None if !moving && kept().speed > 0.2 => [0.0, NUDGE, original[2]],
        None => return,
    };
    for (i, value) in replaced.into_iter().enumerate() {
        e.mem.set_f32(input + 4 * i as u32, value);
    }
    kept().injected = true;
    if logging() {
        eprintln!(
            "Movement: move vector ({:.2}, {:.2}, {:.2}) from the animation -> ({:.2}, {:.2}, {:.2}) from the keys {mode:#x}",
            original[0], original[1], original[2], replaced[0], replaced[1], replaced[2]
        );
    }
}

/// After `MobileObject::Move`: the debug log's line on who set the final velocity.
fn after_move(e: &mut Engine, controller: u32) {
    if !logging() || controller == 0 {
        return;
    }
    let out = [0, 1, 2].map(|i| e.mem.f32(controller + OUT_VELOCITY + 4 * i));
    let current = e.mem.u32(controller + CURRENT_STATE);
    let wrote = kept().wrote;
    let path = match wrote {
        None if tuning().on && crate::gunplay::is_on() => "original (no snappy step this frame)",
        None => "classic",
        Some(w) if (w[0] - out[0]).abs() < 1e-3 && (w[1] - out[1]).abs() < 1e-3 => "snappy",
        Some(_) if out[0] == 0.0 && out[1] == 0.0 => "zeroed after snappy (the controller still)",
        Some(_) => "snappy, then changed by the game (a push, the collision, the jump)",
    };
    eprintln!(
        "Movement: final velocity ({:.2}, {:.2}, {:.2}) state {current}: {path}",
        out[0], out[1], out[2]
    );
}

/// The debug log's line for a step Snappy did not touch.
fn classic_line(e: &mut Engine, output: u32) {
    let Some(controller) = output.checked_sub(OUT_VELOCITY) else {
        return;
    };
    if !is_player(e, controller) {
        return;
    }
    let out = [0, 1, 2].map(|i| e.mem.f32(output + 4 * i));
    eprintln!(
        "Movement: classic step: the util gave ({:.2}, {:.2}, {:.2}) state {}",
        out[0],
        out[1],
        out[2],
        e.mem.u32(controller + CURRENT_STATE)
    );
}

// --- The Settings menu's rows (the Gameplay page). ---------------------------------------------

/// The rows' change functions, after the Test Range's (`0x7e06_00b0`, `0x7e06_00c0`), in the
/// platform's area that no exe or platform code uses.
pub const CHANGE_FNS: [u32; 10] = [
    0x7e06_00d0,
    0x7e06_00e0,
    0x7e06_00f0,
    0x7e06_0100,
    0x7e06_0110,
    0x7e06_0120,
    0x7e06_0130,
    0x7e06_0140,
    0x7e06_0150,
    0x7e06_0160,
];

/// The Gameplay page's mode bit (the original's own first rows, difficulty and the others, have
/// it: `USER_OPTION_ROWS` in `startmenu.rs`).
const GAMEPLAY: u32 = 8;

/// A row of the Settings menu's Gameplay page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// Classic / Snappy: `bSnappyMovement`.
    Mode,
    Accelerate,
    Friction,
    StopSpeed,
    AirAccelerate,
    AirSpeedCap,
    RunSpeed,
    JumpStrength,
    JumpGravity,
    /// No / Yes: every movement value back to its default, at once.
    Reset,
}

/// The rows, in the order they are made.
pub const ROWS: [Row; 10] = [
    Row::Mode,
    Row::Accelerate,
    Row::Friction,
    Row::StopSpeed,
    Row::AirAccelerate,
    Row::AirSpeedCap,
    Row::RunSpeed,
    Row::JumpStrength,
    Row::JumpGravity,
    Row::Reset,
];

impl Row {
    /// The Fallout.ini key the row saves to (`None` for Reset, which saves them all).
    pub fn key(self) -> Option<&'static str> {
        Some(match self {
            Row::Mode => "bSnappyMovement",
            Row::Accelerate => "iAccelerate",
            Row::Friction => "iFriction",
            Row::StopSpeed => "iStopSpeed",
            Row::AirAccelerate => "iAirAccelerate",
            Row::AirSpeedCap => "iAirSpeedCap",
            Row::RunSpeed => "iRunSpeed",
            Row::JumpStrength => "iJumpStrength",
            Row::JumpGravity => "iJumpGravity",
            Row::Reset => return None,
        })
    }

    /// A meter's lowest value, step and number of settings; `None` for a two-value list.
    pub fn meter(self) -> Option<(i32, i32, u32)> {
        match self {
            Row::Accelerate => Some((2, 2, 25)),
            Row::Friction => Some((0, 1, 21)),
            Row::StopSpeed => Some((0, 25, 21)),
            Row::AirAccelerate => Some((0, 5, 21)),
            Row::AirSpeedCap => Some((0, 10, 31)),
            Row::RunSpeed | Row::JumpStrength => Some((50, 5, 31)),
            Row::JumpGravity => Some((50, 10, 26)),
            Row::Mode | Row::Reset => None,
        }
    }

    /// The two values of a list row.
    pub fn choices(self) -> [&'static str; 2] {
        match self {
            Row::Mode => ["Classic", "Snappy"],
            _ => ["No", "Yes"],
        }
    }

    /// The row's value in `tuning`.
    pub fn value_in(self, tuning: &Tuning) -> i32 {
        self.key().and_then(|k| tuning.get(k)).unwrap_or(0)
    }

    /// The setting (meter step or list index) of `value`, and the value of a setting.
    pub fn setting_of(self, value: i32) -> u32 {
        match self.meter() {
            Some((low, step, count)) => {
                ((value - low + step / 2).div_euclid(step)).clamp(0, count as i32 - 1) as u32
            }
            None => u32::from(value != 0),
        }
    }

    pub fn value_of(self, setting: u32) -> i32 {
        match self.meter() {
            Some((low, step, count)) => low + setting.min(count - 1) as i32 * step,
            None => i32::from(setting != 0),
        }
    }

    /// The row's label for `value` (at most 31 characters).
    pub fn label(self, value: i32) -> String {
        match self {
            Row::Mode => "Movement".to_string(),
            Row::Accelerate => format!("Ground Acceleration: {value}"),
            Row::Friction => format!("Ground Friction: {value}"),
            Row::StopSpeed => format!("Stop Speed: {value}"),
            Row::AirAccelerate => format!("Air Control: {value}"),
            Row::AirSpeedCap => format!("Air Speed Cap: {value}"),
            Row::RunSpeed => format!("Run Speed: {value}%"),
            Row::JumpStrength => format!("Jump Strength: {value}%"),
            Row::JumpGravity => format!("Jump Gravity: {value}%"),
            Row::Reset => "Reset Movement Settings".to_string(),
        }
    }
}

/// The rows' options as the menu made them (for Reset, which shows the others' new values).
static OPTIONS: Mutex<[u32; 10]> = Mutex::new([0; 10]);

/// Makes the rows on the Gameplay page and appends them to `pUserOptionsA` (from
/// `crate::gunplay::add_rows`, so they show when the weighty combat row is on).
pub fn add_rows(e: &mut Engine) {
    use crate::gunplay::{
        heap_text, ARRAY_ADD, LABEL_SIZE, LIST, METER, OPERATOR_NEW, USER_OPTIONS, USER_OPTION_NEW,
        USER_OPTION_SIZE,
    };
    let now = tuning();
    for (i, (row, change_fn)) in ROWS.into_iter().zip(CHANGE_FNS).enumerate() {
        let text = heap_text(e, &row.label(row.value_in(&now)), LABEL_SIZE);
        let block = e.call(OPERATOR_NEW, &args![USER_OPTION_SIZE]).u32();
        if text == 0 || block == 0 {
            continue;
        }
        let call = match row.meter() {
            Some((_, _, count)) => args![block, text, change_fn, GAMEPLAY, METER, count, 0u32],
            None => {
                let [a, b] = row.choices();
                let (a, b) = (heap_text(e, a, 12), heap_text(e, b, 12));
                if a == 0 || b == 0 {
                    continue;
                }
                let mut call = args![block, text, change_fn, GAMEPLAY, LIST, 2u32, 0u32];
                call.extend([a, b]);
                call
            }
        };
        let option = e.call(USER_OPTION_NEW, &call).u32();
        OPTIONS.lock().unwrap_or_else(|p| p.into_inner())[i] = option;
        e.with_stack(4, |e, cell| {
            e.mem.set_u32(cell.addr(), option);
            e.call(ARRAY_ADD, &args![USER_OPTIONS, cell]);
        });
    }
}

/// Registers the rows' change functions.
fn install_change_fns(e: &mut Engine) {
    fn row_fn<const N: usize>(e: &mut Engine, a: &[u32]) -> Ret {
        change(e, N, Ptr::new(a[0]));
        Ret::default()
    }
    let fns: [AbiFn; 10] = [
        row_fn::<0>,
        row_fn::<1>,
        row_fn::<2>,
        row_fn::<3>,
        row_fn::<4>,
        row_fn::<5>,
        row_fn::<6>,
        row_fn::<7>,
        row_fn::<8>,
        row_fn::<9>,
    ];
    for (address, f) in CHANGE_FNS.into_iter().zip(fns) {
        e.register(address, f);
    }
}

/// A row's change function: with the menu's save flag clear it reads the value; set, the player
/// moved the row: applied at once (the next physics step uses it) and saved.
fn change(e: &mut Engine, index: usize, option: Ptr) {
    use crate::gunplay::{write_label, DEFAULT_VAL, VAL};
    if option.is_null() {
        return;
    }
    let row = ROWS[index];
    OPTIONS.lock().unwrap_or_else(|p| p.into_inner())[index] = option.addr();
    let now = tuning();
    let Some(menu) = crate::gunplay::menu_saving(e) else {
        e.mem
            .set_u32(option.addr() + VAL, row.setting_of(row.value_in(&now)));
        e.mem.set_u32(
            option.addr() + DEFAULT_VAL,
            row.setting_of(row.value_in(&Tuning::PLAY)),
        );
        write_label(e, 0, option, row.label(row.value_in(&now)));
        return;
    };
    let value = row.value_of(e.mem.u32(option.addr() + VAL));
    let Some(key) = row.key() else {
        // Reset: chosen Yes.
        if value != 0 {
            reset(e, menu);
            e.mem.set_u32(option.addr() + VAL, 0);
        }
        return;
    };
    if value == row.value_in(&now) {
        return;
    }
    let mut changed = now;
    changed.set(key, value);
    set_tuning(changed);
    write_label(e, menu, option, row.label(value));
    let saved = crate::gunplay::save_key(SECTION, key, value);
    eprintln!(
        "Snappy movement: {key} {value}: applied now{}",
        if saved { ", saved" } else { ", not saved" }
    );
}

/// Every movement value back to its default: applied, saved, and the other rows shown with it.
fn reset(e: &mut Engine, menu: u32) {
    use crate::gunplay::{write_label, VAL};
    set_tuning(Tuning::PLAY);
    let mut saved = true;
    for &(key, _, _, _) in &KEYS {
        let value = Tuning::PLAY.get(key).unwrap_or(0);
        saved &= crate::gunplay::save_key(SECTION, key, value);
    }
    let options = *OPTIONS.lock().unwrap_or_else(|p| p.into_inner());
    for (row, option) in ROWS.into_iter().zip(options) {
        if option == 0 || row == Row::Reset {
            continue;
        }
        let value = row.value_in(&Tuning::PLAY);
        e.mem.set_u32(option + VAL, row.setting_of(value));
        write_label(e, menu, Ptr::new(option), row.label(value));
    }
    eprintln!(
        "Snappy movement: every value back to its default{}",
        if saved { ", saved" } else { ", not saved" }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn length(v: [f32; 2]) -> f32 {
        (v[0] * v[0] + v[1] * v[1]).sqrt()
    }

    /// From standing, full speed in about a tenth of a second (Quake's 10), where the original
    /// follows the animation's blend for several tenths.
    #[test]
    fn the_ground_spins_up_fast() {
        let t = Tuning::PLAY;
        let wish = 44.0;
        let mut v = [0.0, 0.0];
        let mut frames = 0;
        while length(v) < 0.9 * wish {
            v = friction(v, DT, t.friction as f32, t.stop_speed as f32 / 7.0);
            v = accelerate(v, [1.0, 0.0], wish, t.accelerate as f32, DT);
            frames += 1;
            assert!(frames < 60, "never spun up");
        }
        assert!(frames as f32 * DT < 0.2, "{} s", frames as f32 * DT);
        // It settles below the wish: friction against acceleration, as in Quake.
        for _ in 0..120 {
            v = friction(v, DT, t.friction as f32, t.stop_speed as f32 / 7.0);
            v = accelerate(v, [1.0, 0.0], wish, t.accelerate as f32, DT);
        }
        assert!(length(v) > 0.85 * wish && length(v) <= wish, "{v:?}");
    }

    /// Letting go stops in a quarter of a second or so; a slow slide stops as fast as from the
    /// stop speed.
    #[test]
    fn the_ground_stops_sharply() {
        let t = Tuning::PLAY;
        let stop = t.stop_speed as f32 / 7.0;
        let mut v = [40.0, 0.0];
        let mut frames = 0;
        while length(v) > 0.0 {
            v = friction(v, DT, t.friction as f32, stop);
            frames += 1;
            assert!(frames < 120, "never stopped");
        }
        assert!(frames as f32 * DT < 0.45, "{} s", frames as f32 * DT);
        let slow = friction([1.0, 0.0], DT, t.friction as f32, stop);
        assert_eq!(slow, [0.0, 0.0], "below the stop speed it is gone at once");
    }

    /// A reversal: from full speed one way to the other side in a few tenths, the speed along the
    /// new direction never beyond the wish.
    #[test]
    fn a_reversal_is_quick() {
        let t = Tuning::PLAY;
        let mut v = [40.0, 0.0];
        let mut frames = 0;
        while v[0] > -30.0 {
            v = friction(v, DT, t.friction as f32, t.stop_speed as f32 / 7.0);
            v = accelerate(v, [-1.0, 0.0], 44.0, t.accelerate as f32, DT);
            frames += 1;
            assert!(frames < 120);
        }
        assert!(frames as f32 * DT < 0.4, "{} s", frames as f32 * DT);
    }

    /// In the air the wish only bends the path: speed along it grows to the cap, sideways speed
    /// stays, and nothing slows the player.
    #[test]
    fn the_air_steers_a_little() {
        let t = Tuning::PLAY;
        let cap = t.air_speed_cap as f32 / 7.0;
        let start = [40.0, 0.0];
        let mut v = start;
        for _ in 0..30 {
            v = air_accelerate(v, [0.0, 1.0], 44.0, cap, t.air_accelerate as f32, DT);
        }
        assert_eq!(v[0], 40.0, "the run-up is kept");
        assert!((v[1] - cap).abs() < 1e-3, "{v:?}");
        // No wish: unchanged.
        assert_eq!(air_accelerate(start, [0.0, 0.0], 0.0, cap, 10.0, DT), start);
    }

    /// A jump as high as the game's under heavier gravity: the take-off speed grows with its
    /// square root (height v^2 / 2g).
    #[test]
    fn the_jump_keeps_its_height() {
        let k = jump_speed_factor(130, 100);
        let (v, g) = (5.0f32, 9.8f32);
        let height = v * v / (2.0 * g);
        let heavier = (k * v) * (k * v) / (2.0 * g * 1.3);
        assert!((height - heavier).abs() < 1e-4);
        assert_eq!(jump_speed_factor(100, 100), 1.0);
        // Jump Strength 150 %: half as high again, whatever the gravity.
        let k = jump_speed_factor(130, 150);
        let higher = (k * v) * (k * v) / (2.0 * g * 1.3);
        assert!((higher - 1.5 * height).abs() < 1e-4);
    }

    /// The util's frame: level ground, facing north, gives the forward and sideways axes level
    /// and the normal up.
    #[test]
    fn the_frame_is_level_on_level_ground() {
        let up = [0.0, 0.0, 1.0, 0.0];
        let [b, c, n] = frame([0.0, 1.0, 0.0, 0.0], up, up).unwrap();
        assert!(b[2].abs() < 1e-6 && c[2].abs() < 1e-6);
        assert!((dot(b, c)).abs() < 1e-6);
        assert_eq!(n, up);
        assert!(frame(up, up, up).is_none(), "forward along up has no frame");
    }

    /// Each row's meter reaches its key's default exactly, stays inside the key's range, and its
    /// label fits the menu's 31 characters.
    #[test]
    fn the_rows_show_every_value() {
        for row in ROWS {
            let value = row.value_in(&Tuning::PLAY);
            assert_eq!(row.value_of(row.setting_of(value)), value, "{row:?}");
            assert!(row.label(value).len() <= 31, "{}", row.label(value));
            if let (Some(key), Some((_, _, count))) = (row.key(), row.meter()) {
                let &(_, _, low, high) = KEYS.iter().find(|k| k.0 == key).unwrap();
                for setting in [0, count - 1] {
                    let v = row.value_of(setting);
                    assert!((low..=high).contains(&v), "{row:?} {v}");
                }
            }
        }
        // Every key has a row (Reset aside), but the debug log's.
        let keyed: Vec<&str> = ROWS.iter().filter_map(|r| r.key()).collect();
        assert_eq!(keyed.len(), KEYS.len() - 1);
        for (key, ..) in KEYS.iter().filter(|k| k.0 != "bMovementLog") {
            assert!(keyed.contains(key), "{key}");
        }
        assert_eq!(Row::Mode.choices(), ["Classic", "Snappy"]);
        assert_eq!(Row::RunSpeed.setting_of(100), 10);
        assert_eq!(Row::Accelerate.value_of(0), 2);
    }

    /// The keys' direction: the stick's when it has one, else the bits; diagonals are unit length,
    /// left is -x, and opposite keys cancel.
    #[test]
    fn the_keys_give_the_direction() {
        assert_eq!(key_direction(0x200, [0.0, 0.0]), None);
        assert_eq!(key_direction(0x1, [0.0, 0.0]), Some([0.0, 1.0]));
        assert_eq!(key_direction(0x4, [0.0, 0.0]), Some([-1.0, 0.0]));
        assert_eq!(
            key_direction(0x8 | 0x2, [0.0, 0.0]).map(|d| d[1] < 0.0 && d[0] > 0.0),
            Some(true)
        );
        let d = key_direction(0x1 | 0x4, [0.0, 0.0]).unwrap();
        assert!((d[0] * d[0] + d[1] * d[1] - 1.0).abs() < 1e-6 && d[0] < 0.0 && d[1] > 0.0);
        assert_eq!(
            key_direction(0x4 | 0x8, [0.0, 0.0]),
            None,
            "left and right cancel"
        );
        assert_eq!(
            key_direction(0x1, [0.0, 0.5]),
            Some([0.0, 1.0]),
            "a stick's own"
        );
        assert_eq!(key_direction(0x1, [0.3, 0.4]), Some([0.6, 0.8]));
    }

    /// The table's values are the built-in tuning, each in its range; variables win and a bad one
    /// stops the start.
    #[test]
    fn the_tuning_reads_the_file_and_the_variables() {
        for &(key, _, low, high) in &KEYS {
            let value = Tuning::PLAY.get(key).unwrap();
            assert!((low..=high).contains(&value), "{key}");
        }
        let t = Tuning::from_parts(&[("iFriction", 4)], |v| {
            (v == "NV_MOVEMENT_ACCELERATE").then(|| "15".to_string())
        })
        .unwrap();
        assert_eq!((t.friction, t.accelerate), (4, 15));
        assert!(Tuning::from_parts(&[], |v| {
            (v == "NV_MOVEMENT_SNAPPY").then(|| "yes".to_string())
        })
        .is_err());
        assert_eq!(parse("bSnappyMovement", "0"), Some(0));
        assert_eq!(parse("iJumpGravity", "20"), None);
    }
}
