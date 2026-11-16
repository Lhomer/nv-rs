//! Ballistic bullets (Patrick, 2026-11-14: Battlefield-style). Part of the weighty combat row;
//! `[nv-rs Gunplay] bBallistics` (Classic hitscan / Projectile), `bPenetration`, `iBulletSpeed`,
//! all three on the Settings menu's Gameplay page. **Not 1:1 with FalloutNV.exe.**
//!
//! **Flight: the game's own.** A bullet's projectile form (`BGSProjectile`, its `DATA` at `+0x60`)
//! is hitscan when bit 0x1 of its flags is set: the shot casts one ray. With it clear the game
//! flies the projectile (`MissileProjectile::UpdateProjectile`), its speed `fSpeed`, its drop
//! `fGravity` (through its character controller), its range `fRange`, shows its model (a hitscan
//! bullet only shows one as a tracer), and the combat AI leads it: `GetProjectedPointForTarget`
//! (`009a8df0`) answers the target's point for hitscan and otherwise iterates on the target's
//! velocity with the flight time from the form's speed and gravity. So in Projectile mode every
//! bullet form a gun of a known calibre uses ([`apply_forms`]: the weapon's `+0x118`, its ammo's
//! override `+0xB4`) is made a flying one with its calibre's speed, drop and range ([`CALIBERS`]);
//! the originals are kept, and Classic (or the row off) puts them back.
//!
//! **Per shot** (over `CreateProjectile`, `009bca60`, which every shot of `TESObjectWEAP::Fire`
//! goes through): the shooter's loaded ammo (`TESObjectWEAP::GetCurrentAmmo`, `00525980`) gives
//! the load ([`Load`]: by its name, else by its effects: a damage threshold multiplied up is a
//! hollow point, one subtracted an armour-piercing round); its velocity factor and the weapon
//! mods' ([`crate::weapon_mods`]: a long or heavy barrel faster, a short one or a silencer slower,
//! the projectile speed effect for anyone but the player, whose shot the game scaled) go into the
//! projectile's speed multiplier (`+0xD0`), and the bullet is remembered (calibre, load, its muzzle
//! speed, whether it is one of several pellets, a split beam's included).
//!
//! **Momentum at impact** ([`bullet_momentum`]): the bullet's mass times its speed where it hits
//! (its muzzle speed, less after each pass; `iBulletSpeed` changes the flight, not the hit), times
//! the load's transfer (a hollow point gives more of
//! it to the body, an armour-piercing round less), in the hit force's damage points
//! ([`POINTS_PER_NEWTON_SECOND`]); the damage threshold counts by the load's factor. The hit force
//! and the knockdowns use it ([`crate::hit_force::Shot::Bullet`]).
//!
//! **Penetration** (over `Projectile::ProcessImpacts`, `009c1b70`): after the game processed a
//! bullet's impact, a round with energy left goes on: through a body ([`BODY_COST`]), or through a
//! thin object of a penetrable material (glass, wood, sheet and hollow metal, cloth, fences:
//! [`material`]: the material's usual thickness in a level, not measured, as the host has no ray
//! cast into the game's world to call yet) it can pass. Each pass costs its share of the round's penetration power
//! ([`Caliber::penetration`] times the load's), the speed and damage going down with it; a
//! continuation is a new projectile of the same form, shooter and weapon, started a little past a
//! thin object or a body's depth past a body at the next frame (never inside the game's impact code), at most [`MAX_PASSES`] passes a bullet and
//! [`MAX_CONTINUATIONS`] a second. A shotgun's pellets never penetrate (a slug can).
//!
//! **Cheap:** one table lookup a shot; nothing per frame but the queued continuations.

use engine::prelude::*;
/// The pure parts (the calibres, the loads, the penetration costs, a round's share left in a body)
/// live in `crates/ragdoll-core`, shared with `tools/ragdoll-lab`.
pub use ragdoll_core::rounds::*;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

// --- The game's words. -------------------------------------------------------------------------

pub const CREATE_PROJECTILE: u32 = 0x009b_ca60;
pub const PROCESS_IMPACTS: u32 = 0x009c_1b70;
const GET_CURRENT_AMMO: u32 = 0x0052_5980;
/// `TESAmmo`'s effect list (`00923000`); a `TESAmmoEffect`'s type (`0059bb30`: 2 is the damage
/// threshold), its operation (`+0x28`: 0 adds, 1 multiplies, 2 subtracts) and value (`00759450`).
const AMMO_EFFECTS: u32 = 0x0092_3000;
const EFFECT_TYPE: u32 = 0x0059_bb30;
const EFFECT_OPERATION: u32 = 0x28;
const EFFECT_VALUE: u32 = 0x0075_9450;
const EFFECT_THRESHOLD: u32 = 2;
/// `BGSProjectile`: flags, gravity, speed, range.
const PROJ_FLAGS: u32 = 0x60;
const PROJ_GRAVITY: u32 = 0x64;
const PROJ_SPEED: u32 = 0x68;
const PROJ_RANGE: u32 = 0x6c;
const HITSCAN: u32 = 0x1;
const TYPE_MASK: u32 = 0x1f_0000;
const TYPE_MISSILE: u32 = 0x1_0000;
/// The projectile instance: its impact list and whether it was processed, its speed multiplier,
/// damage, weapon, shooter, direction; a reference's base form, rotation (x, z).
const IMPACTS: u32 = 0x88;
const IMPACT_PROCESSED: u32 = 0x90;
const SPEED_MULT: u32 = 0xd0;
const DAMAGE: u32 = 0xdc;
const WEAPON: u32 = 0xf8;
const SHOOTER: u32 = 0xfc;
const DIRECTION: u32 = 0x104;
const BASE_FORM: u32 = 0x20;
const ROT_X: u32 = 0x24;
const ROT_Z: u32 = 0x2c;
/// `Projectile::ImpactData`: collidee, location, material.
const IMPACT_COLLIDEE: u32 = 0x00;
const IMPACT_LOCATION: u32 = 0x04;
const IMPACT_MATERIAL: u32 = 0x20;
/// `TESObjectWEAP`: its projectile, its ammo, its projectiles per shot; `TESAmmo`: its projectile
/// override; form types; a form list's first entry.
const WEAPON_PROJECTILE: u32 = 0x118;
const WEAPON_AMMO: u32 = 0xa8;
const WEAPON_PROJECTILES: u32 = 0x11e;
/// `TESObjectWEAP`'s type (`eType`, 3 to 9 guns) and damage.
const WEAPON_KIND: u32 = 0xf4;
const WEAPON_DAMAGE: u32 = 0xa0;
const AMMO_PROJECTILE: u32 = 0xb4;
const WEAP: u8 = 0x28;
const AMMO: u8 = 0x29;
const FLST: u8 = 0x55;
const LIST_FIRST: u32 = 0x18;
/// The player (`PlayerCharacter*` global).
const PLAYER: u32 = 0x011d_ea3c;
pub const UNITS_PER_METRE: f32 = 70.0;
/// Where a continuation starts past a thin object, in units.
const THIN_DEPTH: f32 = 6.0;

/// A converted form's originals (flags, gravity, speed, range) and its calibre.
type Forms = HashMap<u32, ([u32; 4], Caliber)>;
static FORMS: Mutex<Option<Forms>> = Mutex::new(None);

/// A bullet in flight: its calibre, load, passes so far, power left, whether it is a pellet, its
/// speed multiplier and its speed (m/s) at the muzzle, the time it was made.
#[derive(Clone, Copy, Debug)]
struct Bullet {
    caliber: Caliber,
    load: Load,
    passes: u8,
    power: f32,
    pellet: bool,
    muzzle: f32,
    velocity: f32,
    made: Instant,
}

static BULLETS: Mutex<Option<HashMap<u32, Bullet>>> = Mutex::new(None);

/// A continuation waiting for the next frame.
#[derive(Clone, Copy, Debug)]
struct Continuation {
    form: u32,
    shooter: u32,
    weapon: u32,
    at: [f32; 3],
    rot_x: f32,
    rot_z: f32,
    speed_mult: f32,
    damage: f32,
    bullet: Bullet,
}
static QUEUE: Mutex<Vec<Continuation>> = Mutex::new(Vec::new());
static SPAWNED: Mutex<(Option<Instant>, u32)> = Mutex::new((None, 0));

fn on() -> bool {
    crate::gunplay::is_on() && crate::gunplay::tuning().ballistics
}

pub(crate) fn name_of(e: &Engine, form: u32) -> String {
    String::from_utf8_lossy(&jip::commands::the_name(e, form)).into_owned()
}

/// An ammo, or a form list's first ammo.
pub(crate) fn ammo_form(e: &Engine, form: u32) -> u32 {
    if form == 0 {
        return 0;
    }
    match e.mem.u8(form + 4) {
        AMMO => form,
        FLST => {
            let first = e.mem.u32(form + LIST_FIRST);
            if first != 0 && e.mem.u8(first + 4) == AMMO {
                first
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// Every bullet form of the loaded guns made a flying one (Projectile mode) or put back
/// (module documentation). Run when the main menu is made and when a row changes.
pub fn apply_forms(e: &mut Engine) {
    let t = crate::gunplay::tuning();
    let flying = on();
    let mut guard = FORMS.lock().unwrap_or_else(|p| p.into_inner());
    let forms = guard.get_or_insert_with(HashMap::new);
    // Put back first, so a speed change starts from the originals.
    for (&form, &(orig, _)) in forms.iter() {
        for (i, word) in orig.into_iter().enumerate() {
            e.mem.set_u32(form + PROJ_FLAGS + 4 * i as u32, word);
        }
    }
    if !flying {
        if !forms.is_empty() {
            eprintln!(
                "Ballistics: {} bullet projectiles back to hitscan (Classic)",
                forms.len()
            );
        }
        forms.clear();
        return;
    }
    let mut found: Vec<(u32, Caliber)> = Vec::new();
    for weapon in crate::test_range::all_forms(e) {
        if e.mem.u8(weapon + 4) != WEAP {
            continue;
        }
        let ammo = ammo_form(e, e.mem.u32(weapon + WEAPON_AMMO));
        if ammo == 0 {
            continue;
        }
        let Some(caliber) = caliber_of(&name_of(e, ammo)) else {
            continue;
        };
        for projectile in [
            e.mem.u32(weapon + WEAPON_PROJECTILE),
            e.mem.u32(ammo + AMMO_PROJECTILE),
        ] {
            if projectile != 0 {
                found.push((projectile, caliber));
            }
        }
    }
    // The per-gun velocity's fallback (each calibre's average damage) worked out now, not at a
    // shot.
    crate::gun_velocity::prepare(e);
    let scale = t.bullet_speed.clamp(1, 1000) as f32 / 100.0;
    for (form, caliber) in found {
        let orig = match forms.get(&form) {
            Some(&(orig, _)) => orig,
            None => [0, 4, 8, 12].map(|o| e.mem.u32(form + PROJ_FLAGS + o)),
        };
        if orig[0] & HITSCAN == 0 || orig[0] & TYPE_MASK != TYPE_MISSILE {
            continue;
        }
        forms.insert(form, (orig, caliber));
        let (speed, gravity, range) = form_flight(caliber, scale, f32::from_bits(orig[3]));
        e.mem.set_u32(form + PROJ_FLAGS, orig[0] & !HITSCAN);
        e.mem.set_f32(form + PROJ_GRAVITY, gravity);
        e.mem.set_f32(form + PROJ_SPEED, speed);
        e.mem.set_f32(form + PROJ_RANGE, range);
    }
    eprintln!(
        "Ballistics: {} bullet projectiles fly (Projectile mode, speed {}%)",
        forms.len(),
        t.bullet_speed
    );
}

/// A calibre's form speed (units a second), gravity and range (units, never below the form's own
/// `range`), at `scale` times the speed.
pub fn form_flight(caliber: Caliber, scale: f32, range: f32) -> (f32, f32, f32) {
    let speed = (caliber.velocity * AVERAGE_SPEED * UNITS_PER_METRE * scale).min(MAX_FORM_SPEED);
    let range = if range.is_finite() { range } else { 0.0 };
    (
        speed,
        caliber.drop,
        range.max(caliber.range * UNITS_PER_METRE),
    )
}

/// The ammo effects' damage threshold multiplier (1 for none) and subtraction (0 for none).
pub(crate) fn threshold_effects(e: &mut Engine, ammo: u32) -> (f32, f32) {
    let (mut mult, mut minus) = (1.0f32, 0.0f32);
    let mut node = e.call(AMMO_EFFECTS, &args![ammo]).u32();
    for _ in 0..16 {
        if node == 0 {
            break;
        }
        let effect = e.mem.u32(node);
        if effect != 0 && e.call(EFFECT_TYPE, &args![effect]).u32() == EFFECT_THRESHOLD {
            let value = e.call(EFFECT_VALUE, &args![effect]).f64() as f32;
            if value.is_finite() {
                match e.mem.u32(effect + EFFECT_OPERATION) {
                    1 => mult *= value,
                    2 => minus += value,
                    0 => minus -= value,
                    _ => {}
                }
            }
        }
        node = e.mem.u32(node + 4);
    }
    (mult, minus)
}

/// Registers the hooks (from `crate::gunplay::install`).
pub fn install(e: &mut Engine) {
    e.register(CREATE_PROJECTILE, |e, a| {
        let (_, create) = engine::entry!(
            0x009bca60,
            engine::units::fallout_ai::projectile::fn_009bca60(
                Ptr, u32, u32, u32, u32, u32, u32, f32, f32, u32, u32, u8, u8, f32, f32, u32
            ) -> Ptr<engine::units::fallout_ai::projectile::Projectile>
        );
        // A two-handed weapon fired with one hand goes wider (`crate::one_hand`).
        let mut args = a.to_vec();
        if crate::gunplay::is_on() {
            crate::one_hand::widen(e, &mut args);
        }
        let made = create(e, &args);
        if a.len() >= 4 && on() {
            shot(e, a[0], a[1], a[3], made.eax);
        }
        made
    });
    e.register(PROCESS_IMPACTS, |e, a| {
        use engine::units::fallout_ai::projectile::projectile_process_impacts;
        let projectile = a[0];
        let first = if on() {
            first_impact(e, projectile)
        } else {
            None
        };
        let processed = projectile_process_impacts(e, Ptr::new(projectile));
        if processed {
            if let Some(impact) = first {
                penetrate(e, projectile, impact);
            }
        }
        processed.into_ret()
    });
}

/// `TESObjectWEAP::GetCurrentAmmo` (`00525980`) for `weapon` as `holder` has it loaded, asked
/// with no holder unless `holder` is an actor: the weapon's default ammo then. The game's function
/// takes its second word for an actor and reads its process (`+0x68`); the game's own fire asks
/// it only for an actor (`TESObjectWEAP::Fire` checks), but every shot's projectile came here
/// with its shooter as it was: a booby-trapped terminal's, a mine's, a turret's reference.
/// Patrick, 2026-11-17: a rigged terminal's blast in REPCONN's basement stopped the game (a read
/// of unmapped memory at `0048385c` in `00525980` under `009bca60`).
pub(crate) fn current_ammo(e: &mut Engine, weapon: u32, holder: u32) -> u32 {
    let holder = if jip::commands::is_actor(e, holder) {
        holder
    } else {
        0
    };
    e.call(GET_CURRENT_AMMO, &args![weapon, holder]).u32()
}

/// A new shot's projectile: its load's velocity, and the bullet remembered.
fn shot(e: &mut Engine, form: u32, shooter: u32, weapon: u32, projectile: u32) {
    if projectile == 0 || weapon == 0 {
        return;
    }
    let caliber = {
        let guard = FORMS.lock().unwrap_or_else(|p| p.into_inner());
        match guard.as_ref().and_then(|f| f.get(&form)) {
            Some(&(_, c)) => c,
            None => return,
        }
    };
    let ammo = current_ammo(e, weapon, shooter);
    let ammo = ammo_form(e, ammo);
    let load = if ammo == 0 {
        Load::Standard
    } else {
        let (mult, minus) = threshold_effects(e, ammo);
        load_of(&name_of(e, ammo), mult, minus)
    };
    let mods = load.mods();
    // The weapon mods on the shooter's gun: barrels, a silencer, a split beam's projectiles, and
    // (for anyone but the player, whose shot the game scaled) the projectile speed effect.
    let weapon_mods = crate::weapon_mods::on_weapon(e, shooter, weapon);
    let player = e.global::<u32>(PLAYER);
    let fitted = crate::weapon_mods::velocity_factor(&weapon_mods, shooter != player);
    let given = e.mem.f32(projectile + SPEED_MULT);
    let given = if given.is_finite() && given > 0.0 {
        given
    } else {
        1.0
    };
    // The gun's own multiplier (`crate::gun_velocity`), and the penetration with the speed: the
    // round's energy goes with the square of its speed.
    let gun = crate::gun_velocity::multiplier(e, weapon).0;
    let muzzle = given * mods.velocity * fitted * gun;
    let faster = (mods.velocity * fitted * gun).clamp(0.25, 4.0);
    e.mem.set_f32(projectile + SPEED_MULT, muzzle);
    let projectiles = u32::from(e.mem.u8(weapon + WEAPON_PROJECTILES))
        + crate::weapon_mods::extra_projectiles(&weapon_mods);
    let pellet = projectiles > 1 && load != Load::Slug;
    remember(
        projectile,
        Bullet {
            caliber,
            load,
            passes: 0,
            power: caliber.penetration * mods.penetration * faster * faster,
            pellet,
            muzzle,
            velocity: caliber.velocity * muzzle,
            made: Instant::now(),
        },
    );
}

fn remember(projectile: u32, bullet: Bullet) {
    let mut guard = BULLETS.lock().unwrap_or_else(|p| p.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if map.len() > 512 {
        map.retain(|_, b| b.made.elapsed().as_secs() < 10);
    }
    map.insert(projectile, bullet);
}

fn bullet(projectile: u32) -> Option<Bullet> {
    BULLETS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()?
        .get(&projectile)
        .copied()
}

/// The momentum of the bullet `projectile` where it hits, in the hit force's points, and the
/// factor the damage threshold counts by; `None` for no remembered bullet (Classic, energy).
pub fn bullet_momentum(e: &Engine, projectile: u32) -> Option<(f32, f32)> {
    if projectile == 0 || !on() {
        return None;
    }
    let b = bullet(projectile)?;
    let mods = b.load.mods();
    // Its speed now against its speed at the muzzle: less after each pass.
    let now = e.mem.f32(projectile + SPEED_MULT);
    let share = if b.muzzle > 0.0 && now.is_finite() {
        (now / b.muzzle).clamp(0.0, 1.5)
    } else {
        1.0
    };
    let speed = b.velocity * share;
    Some((
        momentum_points(b.caliber.mass * mods.mass, speed) * mods.transfer,
        mods.threshold,
    ))
}

/// The round of the remembered bullet `projectile` (Projectile mode): its speed now against its
/// speed at the muzzle (less after each pass); `None` for none (Classic, energy).
pub fn bullet_round(e: &Engine, projectile: u32) -> Option<Round> {
    if projectile == 0 || !on() {
        return None;
    }
    let b = bullet(projectile)?;
    let mods = b.load.mods();
    let now = e.mem.f32(projectile + SPEED_MULT);
    let left = if b.muzzle > 0.0 && now.is_finite() {
        (now / b.muzzle).clamp(0.0, 1.5)
    } else {
        1.0
    };
    Some(Round {
        mass: b.caliber.mass * mods.mass / 1000.0,
        speed: b.velocity * left,
        share: transfer_share(b.load, b.pellet),
        threshold: mods.threshold,
        pellet: b.pellet,
    })
}

/// The round `shooter`'s `weapon` fires now (Classic hitscan, or a bullet not remembered): its
/// loaded ammo's calibre and load, the weapon mods' velocity; `None` for no calibre (energy,
/// flame, launchers, melee).
pub fn weapon_round(e: &mut Engine, weapon: u32, shooter: u32) -> Option<Round> {
    if weapon == 0 || e.mem.u8(weapon + 4) != WEAP {
        return None;
    }
    let current = current_ammo(e, weapon, shooter);
    let mut ammo = ammo_form(e, current);
    if ammo == 0 {
        ammo = ammo_form(e, e.mem.u32(weapon + WEAPON_AMMO));
    }
    if ammo == 0 {
        return None;
    }
    let caliber = caliber_of(&name_of(e, ammo))?;
    let (mult, minus) = threshold_effects(e, ammo);
    let load = load_of(&name_of(e, ammo), mult, minus);
    let mods = load.mods();
    let weapon_mods = crate::weapon_mods::on_weapon(e, shooter, weapon);
    let fitted = crate::weapon_mods::velocity_factor(&weapon_mods, true);
    let projectiles = u32::from(e.mem.u8(weapon + WEAPON_PROJECTILES))
        + crate::weapon_mods::extra_projectiles(&weapon_mods);
    let pellet = projectiles > 1 && load != Load::Slug;
    Some(Round {
        mass: caliber.mass * mods.mass / 1000.0,
        speed: caliber.velocity
            * mods.velocity
            * fitted
            * crate::gun_velocity::multiplier(e, weapon).0,
        share: transfer_share(load, pellet),
        threshold: mods.threshold,
        pellet,
    })
}

/// A gun's calibre (by its default ammo) and its damage, `None` for a weapon of no calibre (melee,
/// energy, launchers, thrown).
pub fn gun_caliber(e: &mut Engine, weapon: u32) -> Option<(Caliber, f32)> {
    if weapon == 0 || e.mem.u8(weapon + 4) != WEAP {
        return None;
    }
    let kind = e.mem.i8(weapon + WEAPON_KIND);
    if !(3..=9).contains(&kind) {
        return None;
    }
    let ammo = ammo_form(e, e.mem.u32(weapon + WEAPON_AMMO));
    if ammo == 0 {
        return None;
    }
    let caliber = caliber_of(&name_of(e, ammo))?;
    Some((caliber, f32::from(e.mem.u16(weapon + WEAPON_DAMAGE))))
}

/// A gun's round, told factor by factor (the detailed stats, `crate::item_details`).
#[derive(Clone, Debug)]
pub struct Breakdown {
    pub caliber: Caliber,
    pub ammo_name: String,
    pub load: Load,
    pub gun: f32,
    pub gun_source: crate::gun_velocity::Source,
    /// Each mod with a velocity factor: its name and factor.
    pub mods: Vec<(String, f32)>,
    /// m/s at the muzzle, the bullet's mass (kg), its projectiles a shot.
    pub velocity: f32,
    pub mass: f32,
    pub projectiles: u32,
    pub pellet: bool,
    /// The penetration power and the share left in a body, the DT factor.
    pub power: f32,
    pub share: f32,
    pub threshold: f32,
}

/// The breakdown of `weapon` as `item` (an `ItemChange`, 0 for none) fires it, with the ammo
/// `holder` has loaded (the player's, else its default): `None` for a weapon of no calibre.
pub fn breakdown(e: &mut Engine, weapon: u32, item: u32, holder: u32) -> Option<Breakdown> {
    gun_caliber(e, weapon)?;
    let current = current_ammo(e, weapon, holder);
    let mut ammo = ammo_form(e, current);
    if ammo == 0 {
        ammo = ammo_form(e, e.mem.u32(weapon + WEAPON_AMMO));
    }
    let ammo_name = name_of(e, ammo);
    let caliber = caliber_of(&ammo_name).or_else(|| gun_caliber(e, weapon).map(|c| c.0))?;
    let (mult, minus) = threshold_effects(e, ammo);
    let load = load_of(&ammo_name, mult, minus);
    let mods = load.mods();
    let (gun, gun_source) = crate::gun_velocity::multiplier(e, weapon);
    let attached = crate::weapon_mods::attached(e, item);
    let mut named = Vec::new();
    for m in &attached {
        let f = crate::weapon_mods::mod_velocity(m, true);
        if (f - 1.0).abs() > 1e-4 {
            let name = if m.object == 0 {
                "a mod".to_string()
            } else {
                name_of(e, m.object)
            };
            named.push((name, f));
        }
    }
    let fitted = crate::weapon_mods::velocity_factor(&attached, true);
    let projectiles = u32::from(e.mem.u8(weapon + WEAPON_PROJECTILES)).max(1)
        + crate::weapon_mods::extra_projectiles(&attached);
    let pellet = projectiles > 1 && load != Load::Slug;
    let faster = (mods.velocity * fitted * gun).clamp(0.25, 4.0);
    Some(Breakdown {
        caliber,
        ammo_name,
        load,
        gun,
        gun_source,
        mods: named,
        velocity: caliber.velocity * mods.velocity * fitted * gun,
        mass: caliber.mass * mods.mass / 1000.0,
        projectiles,
        pellet,
        power: caliber.penetration * mods.penetration * faster * faster,
        share: transfer_share(load, pellet),
        threshold: mods.threshold,
    })
}

/// The projectile's first impact, when it has not been processed yet: collidee, location,
/// material.
fn first_impact(e: &Engine, projectile: u32) -> Option<(u32, [f32; 3], u32)> {
    if e.mem.u8(projectile + IMPACT_PROCESSED) != 0 || bullet(projectile).is_none() {
        return None;
    }
    let data = e.mem.u32(projectile + IMPACTS);
    if data == 0 {
        return None;
    }
    let at = [0, 4, 8].map(|o| e.mem.f32(data + IMPACT_LOCATION + o));
    Some((
        e.mem.u32(data + IMPACT_COLLIDEE),
        at,
        e.mem.u32(data + IMPACT_MATERIAL),
    ))
}

/// After the game processed an impact: a continuation past it when the round has power left
/// (module documentation).
fn penetrate(e: &mut Engine, projectile: u32, (collidee, at, material_id): (u32, [f32; 3], u32)) {
    let Some(b) = bullet(projectile) else {
        return;
    };
    if !crate::gunplay::tuning().penetration || b.pellet || b.passes >= MAX_PASSES {
        return;
    }
    if !at.iter().all(|v| v.is_finite()) {
        return;
    }
    let d = [0, 4, 8].map(|o| e.mem.f32(projectile + DIRECTION + o));
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if length.is_nan() || length <= 1e-3 {
        return;
    }
    let d = d.map(|v| v / length);
    let body = collidee != 0 && e.vcall(collidee, 0x100, &args![]).bool();
    let (cost, depth) = if body {
        if !b.load.over_penetrates() {
            return;
        }
        (BODY_COST, BODY_DEPTH)
    } else {
        match material_cost(material_id) {
            Some(cost) => (cost, THIN_DEPTH),
            None => return,
        }
    };
    let Some(left) = after_pass(b.power, cost) else {
        return;
    };
    let keep = (left / b.power).sqrt();
    let next = Continuation {
        form: e.mem.u32(projectile + BASE_FORM),
        shooter: e.mem.u32(projectile + SHOOTER),
        weapon: e.mem.u32(projectile + WEAPON),
        at: [0, 1, 2].map(|i| at[i] + d[i] * depth),
        rot_x: e.mem.f32(projectile + ROT_X),
        rot_z: e.mem.f32(projectile + ROT_Z),
        speed_mult: e.mem.f32(projectile + SPEED_MULT) * keep,
        damage: e.mem.f32(projectile + DAMAGE) * keep * keep,
        bullet: Bullet {
            passes: b.passes + 1,
            power: left,
            made: Instant::now(),
            ..b
        },
    };
    if next.form == 0 {
        return;
    }
    QUEUE.lock().unwrap_or_else(|p| p.into_inner()).push(next);
}

/// The bullets in flight and the queued continuations dropped (a load, a new game, the main menu:
/// their projectiles, shooters and weapons' references go with the game).
pub fn clear() {
    if let Some(map) = BULLETS.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        map.clear();
    }
    QUEUE.lock().unwrap_or_else(|p| p.into_inner()).clear();
}

/// Once a frame: the queued continuations made (at most [`MAX_CONTINUATIONS`] a second).
pub fn frame(e: &mut Engine) {
    let queued = std::mem::take(&mut *QUEUE.lock().unwrap_or_else(|p| p.into_inner()));
    for c in queued {
        {
            let mut s = SPAWNED.lock().unwrap_or_else(|p| p.into_inner());
            if s.0.map_or(true, |t| t.elapsed().as_secs() >= 1) {
                *s = (Some(Instant::now()), 0);
            }
            if s.1 >= MAX_CONTINUATIONS {
                continue;
            }
            s.1 += 1;
        }
        let made = e
            .call(
                CREATE_PROJECTILE,
                &args![
                    c.form, c.shooter, 0u32, c.weapon, c.at[0], c.at[1], c.at[2], c.rot_z, c.rot_x,
                    0u32, 0u32, 0u32, 0u32, 0.0f32, 0.0f32, 0u32
                ],
            )
            .u32();
        if made != 0 {
            e.mem.set_f32(made + SPEED_MULT, c.speed_mult);
            e.mem.set_f32(made + DAMAGE, c.damage);
            remember(made, c.bullet);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The current ammo is asked for an actor as it is, and for anything else (a rigged terminal,
    /// a mine, no shooter) with no holder: the game's function reads an actor's process from it.
    #[test]
    fn a_trap_shot_asks_for_no_holders_ammo() {
        let mut e = Engine::new();
        e.mem.map(0x3000, 0x1000);
        let asked = std::sync::Arc::new(Mutex::new(Vec::new()));
        let seen = asked.clone();
        e.register_double(GET_CURRENT_AMMO, move |_, a| {
            seen.lock().unwrap().push(a[1]);
            0x0a44u32.into_ret()
        });
        // An actor: its vtable's slot 0x100 is `Actor::IsActor` (`008d0360`).
        let (actor, terminal) = (0x3000u32, 0x3100u32);
        let (actor_vtable, ref_vtable) = (0x3800u32, 0x3900u32);
        e.mem.set_u32(actor, actor_vtable);
        e.mem.set_u32(actor_vtable + 0x100, 0x008d_0360);
        // A terminal: a reference's vtable, its slot 0x100 another function.
        e.mem.set_u32(terminal, ref_vtable);
        e.mem.set_u32(ref_vtable + 0x100, 0x0040_1234);
        let weapon = 0x5000u32;
        assert_eq!(current_ammo(&mut e, weapon, actor), 0x0a44);
        assert_eq!(current_ammo(&mut e, weapon, terminal), 0x0a44);
        assert_eq!(current_ammo(&mut e, weapon, 0), 0x0a44);
        assert_eq!(*asked.lock().unwrap(), vec![actor, 0, 0]);
    }

    /// The forms fly at their calibre's speed (times the slider), drop by their calibre and reach
    /// at least as far as before; momentum is mass times speed.
    #[test]
    fn forms_fly_by_calibre() {
        let five = caliber_of("5.56mm Round").unwrap();
        let (speed, gravity, range) = form_flight(five, 1.0, 5000.0);
        assert!((speed - 990.0 * AVERAGE_SPEED * UNITS_PER_METRE).abs() < 1.0);
        assert_eq!(gravity, five.drop);
        assert_eq!(range, five.range * UNITS_PER_METRE);
        assert_eq!(form_flight(five, 1.0, 1e9).2, 1e9, "never shorter");
        assert_eq!(form_flight(five, 0.5, 0.0).0, speed / 2.0);
        let gauss = caliber_of("2mm Electromagnetic Cartridge").unwrap();
        assert_eq!(form_flight(gauss, 2.0, 0.0).0, MAX_FORM_SPEED);
        assert!((momentum_points(4.0, 990.0) - 23.76).abs() < 0.01);
        assert!(momentum_points(26.0, 480.0) > 3.0 * momentum_points(4.0, 990.0));
    }
}
