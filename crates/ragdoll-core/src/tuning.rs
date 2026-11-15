//! Every tuning value of the living-body physics by name: the constants of [`crate::flinch`],
//! [`crate::balance`], [`crate::knock`] and [`crate::rounds`], and the game's `[nv-rs Gunplay]`
//! ini settings that scale them.
//!
//! **The override.** The pure functions read their values through [`get`]. Until [`set`] is called
//! that is [`Tuning::GAME`], the constants as written, with no lock and no copy; the host never
//! calls [`set`], so the game runs on the constants exactly. `tools/ragdoll-lab` calls it from its
//! sliders.
//!
//! **Text.** [`Tuning::to_ini`] writes the values as ini text: the game's settings under
//! `[nv-rs Gunplay]` with their ini keys (paste that section into `nv-rs.ini`), the compiled
//! constants under `[nv-rs Ragdoll Lab]` (the game reads no such section; change the constant in
//! this crate to carry one over). [`Tuning::from_ini`] reads either back.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use crate::{balance as b, flinch as f, knock as k, rounds as r};

/// One role's spring frequency (Hz) and damping ratio, its share of a kick and its limit (rad).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoleTune {
    pub hz: f32,
    pub zeta: f32,
    pub gain: f32,
    pub limit: f32,
}

/// What a tuning value is, for the lab's sliders and the text: its name, its group, its range and
/// step, the game's ini key when it is an ini setting, and a line about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub group: &'static str,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub ini: Option<&'static str>,
    pub doc: &'static str,
}

/// The ini section of the game's settings, and the lab's section for the compiled constants.
pub const GAME_SECTION: &str = "nv-rs Gunplay";
pub const LAB_SECTION: &str = "nv-rs Ragdoll Lab";

macro_rules! tuning {
    ($( $group:literal { $( $field:ident = $default:expr, $min:expr, $max:expr, $step:expr, $ini:expr, $doc:literal; )* } )*) => {
        /// Every tuning value (module documentation).
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Tuning {
            $( $( #[doc = $doc] pub $field: f32, )* )*
            /// Each role's spring, gain and limit ([`crate::flinch::ROLES`]' order).
            pub roles: [RoleTune; 11],
        }

        impl Tuning {
            /// The game's values: the constants as written.
            pub const GAME: Tuning = Tuning {
                $( $( $field: $default as f32, )* )*
                roles: game_roles(),
            };

            /// The scalar values' descriptions, in order.
            fn scalars() -> Vec<Field> {
                vec![ $( $( Field {
                    name: stringify!($field).to_string(),
                    group: $group,
                    min: $min as f32,
                    max: $max as f32,
                    step: $step as f32,
                    ini: $ini,
                    doc: $doc,
                }, )* )* ]
            }

            fn get_scalar(&self, name: &str) -> Option<f32> {
                match name {
                    $( $( stringify!($field) => Some(self.$field), )* )*
                    _ => None,
                }
            }

            fn set_scalar(&mut self, name: &str, value: f32) -> bool {
                match name {
                    $( $( stringify!($field) => { self.$field = value; true } )* )*
                    _ => false,
                }
            }
        }
    };
}

const fn game_roles() -> [RoleTune; 11] {
    let mut out = [RoleTune {
        hz: 0.0,
        zeta: 0.0,
        gain: 0.0,
        limit: 0.0,
    }; 11];
    let mut i = 0;
    while i < 11 {
        out[i] = RoleTune {
            hz: f::SPRINGS[i].0,
            zeta: f::SPRINGS[i].1,
            gain: f::GAINS[i],
            limit: f::LIMITS[i],
        };
        i += 1;
    }
    out
}

tuning! {
    "Game settings" {
        hit_force = 100, 0, 300, 1, Some("iHitForce"), "Hit Force (%): the body's speed is the stacked momentum over its mass times this.";
        knockdowns = 1, 0, 1, 1, Some("bKnockdowns"), "Knockdowns on (1) or off (0).";
        knockdown_threshold = 100, 25, 400, 1, Some("iKnockdownThreshold"), "Knockdown Threshold (%) of the speed that topples the body.";
        impact_stacking = 300, 100, 1000, 10, Some("iImpactStacking"), "Impact Stacking: the stack's half-life (ms).";
        hit_reactions = 100, 0, 200, 1, Some("iHitReactions"), "Hit Reactions (%): the flinch's strength; 0 = off.";
        balance = 100, 0, 200, 1, Some("iBalance"), "Balance (%): the push on the centre of mass for catch steps; 0 = off.";
    }
    "Flinch" {
        kick = f::KICK, 0, 60, 0.5, None, "Kick: rad/s of a bone's spin per m/s of the body's speed.";
        spread_base = f::SPREAD_BASE, 0.05, 1.5, 0.01, None, "The kick's reach (m) at no speed.";
        spread_per_mps = f::SPREAD_PER_MPS, 0, 2, 0.01, None, "The kick's reach added per m/s (m).";
        spread_max = f::SPREAD_MAX, 0.1, 3, 0.05, None, "The kick's reach at most (m).";
        muscle_dip = f::MUSCLE_DIP, 0, 0.9, 0.01, None, "How much the muscles near the hit go slack.";
        muscle_back = f::MUSCLE_BACK, 0.02, 2, 0.01, None, "How fast the slack muscles come back (s).";
        head_lag = f::HEAD_LAG, 0, 2, 0.05, None, "The head's lag: a share of the spine's kick against it.";
        knee_buckle = f::KNEE_BUCKLE, 0, 20, 0.1, None, "A leg hit's knee buckle (rad/s per m/s).";
        pelvis_drop = f::PELVIS_DROP, 0, 3, 0.05, None, "A leg hit's pelvis drop (m/s per m/s).";
        pelvis_drop_max = f::PELVIS_DROP_MAX, 0, 0.5, 0.01, None, "The pelvis's drop at most (m).";
    }
    "Balance" {
        foot_reach = b::FOOT_REACH, 0, 0.6, 0.01, None, "The support around the feet (m): a capture point within it needs no step.";
        step_length = b::STEP_LENGTH, 0.1, 1.5, 0.01, None, "A catch step's length at most (m).";
        max_steps = b::MAX_STEPS, 1, 8, 1, None, "The most catch steps.";
        stance = b::STANCE, 0, 0.4, 0.01, None, "How far apart across the push the feet land (m).";
        step_time = b::STEP_TIME, 0.05, 1, 0.01, None, "A full step's time (s).";
        min_step_time = b::MIN_STEP_TIME, 0.03, 0.6, 0.01, None, "A step's time at least (s).";
        swing_height = b::SWING_HEIGHT, 0, 0.4, 0.01, None, "The swinging foot's lift (m).";
        lean = b::LEAN, 0, 0.4, 0.01, None, "How far the pelvis leans past the feet while stepping (m).";
        step_drop = b::STEP_DROP, 0, 0.3, 0.01, None, "How far the pelvis drops while stepping (m).";
        arms_out = b::ARMS_OUT, 0, 2, 0.05, None, "The arms raised while stepping (rad).";
        blend_in = b::BLEND_IN, 0.01, 1, 0.01, None, "The steps' blend in (s).";
        blend_out = b::BLEND_OUT, 0.01, 2, 0.01, None, "The steps' blend back to the animation (s).";
        ground_across = b::GROUND_ACROSS, 0, 100, 1, None, "How far across the ground point may be from a step's target (units).";
        step_up = b::STEP_UP, 0, 120, 1, None, "How far up or down a foot may land (units).";
        ledge = b::LEDGE, 10, 300, 1, None, "A ledge's drop (units).";
        max_time = b::MAX_TIME, 0.5, 10, 0.1, None, "A stepping stops after this whatever it does (s).";
    }
    "Brace and fall" {
        brace_reach = f::BRACE_REACH, 0.02, 1, 0.01, None, "The wall brace's reach (s).";
        brace_hold = f::BRACE_HOLD, 0, 3, 0.05, None, "The wall brace's hold (s).";
        brace_release = f::BRACE_RELEASE, 0.02, 2, 0.01, None, "The wall brace's release (s).";
        brace_holds = f::BRACE_HOLDS, 0, 10, 0.1, None, "The push a wall brace holds (m/s); past it the body crumples.";
        fall_lead = f::FALL_LEAD, 0, 1, 0.01, None, "A knockdown's lead: the posed body goes over this long before the ragdoll (s).";
        ledge_lead = f::LEDGE_LEAD, 0, 2, 0.01, None, "A ledge fall's lead, windmilling (s).";
        overrun_lead = f::OVERRUN_LEAD, 0, 1, 0.01, None, "The lead of a fall at the end of the steps (s).";
        windmill_hz = f::WINDMILL_HZ, 0, 6, 0.1, None, "The windmill's turns a second.";
        windmill_swing = f::WINDMILL_SWING, 0, 3, 0.05, None, "The windmill's swing (rad).";
        fall_lean = f::FALL_LEAN, 0, 1.5, 0.01, None, "How far the torso goes over in a fall (rad).";
        fall_knees = f::FALL_KNEES, 0, 1.5, 0.01, None, "How far the knees give in a fall (rad).";
        fall_shift = f::FALL_SHIFT, 0, 80, 1, None, "How far the pelvis goes in a fall's lead (units).";
        ground_brace_for = f::BRACE_GROUND_SECONDS, 0, 3, 0.05, None, "Bracing on the ground: how long after the ragdoll takes the body (s).";
        ground_brace_height = f::BRACE_GROUND_HEIGHT, 0, 80, 1, None, "Bracing on the ground while the pelvis is this high over the lowest bone (units).";
        ground_brace_drop = f::BRACE_GROUND_DROP, 0, 200, 1, None, "... and dropping faster than this (units a second).";
        ground_brace_share = f::BRACE_GROUND_SHARE, 0, 1, 0.01, None, "The share a frame the hands and forearms are steered.";
        ground_brace_along = f::BRACE_GROUND_SPEED.0, 0, 6, 0.1, None, "The arms' speed along the fall (m/s).";
        ground_brace_down = f::BRACE_GROUND_SPEED.1, 0, 8, 0.1, None, "The arms' speed down (m/s).";
    }
    "Knockdown" {
        recovery = k::RECOVERY, 0.5, 10, 0.1, None, "How much faster than the bare tip-over speed a body can be pushed before it cannot step to catch itself.";
        stagger_share = k::STAGGER_SHARE, 0, 1, 0.01, None, "A stagger at this share of the topple speed.";
        bullet_feel = k::BULLET_FEEL, 0, 60, 0.5, None, "A round's momentum is taken this many times.";
        share_through = r::SHARE_THROUGH, 0.05, 1, 0.01, None, "A standard round's share of its momentum left in the body.";
        damage_momentum = k::DAMAGE_MOMENTUM, 0, 2, 0.01, None, "N s per damage point of a gun of no known calibre.";
        arm_mass = k::ARM_MASS, 0, 20, 0.1, None, "The arm behind a swing (kg).";
        swing_speed = k::SWING_SPEED, 0, 30, 0.1, None, "A swing's speed (m/s) before Strength, skill, reach, power.";
        melee_share = k::MELEE_SHARE, 0, 1, 0.01, None, "The share of a swing's momentum left in the body.";
        power_attack = k::POWER_ATTACK, 1, 4, 0.05, None, "A power attack's speed factor.";
        blast_per_point = k::BLAST_PER_POINT, 0, 20, 0.1, None, "N s per point of a blast's damage there.";
        armour_dt = k::ARMOUR_DT, 1, 200, 1, None, "Armour keeps 1 / (1 + DT / this) of a round's or swing's push.";
        max_launch = k::MAX_LAUNCH, 1, 40, 0.5, None, "The launch at most (m/s).";
        lift = k::LIFT, 0, 2, 0.01, None, "The launch's lift (added upward to the hit's direction).";
    }
    "Bodies" {
        human_mass = k::HUMAN.mass, 20, 300, 1, None, "A human's mass (kg).";
        human_height = k::HUMAN.height, 0.3, 2, 0.01, None, "A human's centre of mass height (m).";
        human_stance = k::HUMAN.stance, 0.05, 0.6, 0.01, None, "Half a human's stance (m).";
        mutant_mass = k::SUPER_MUTANT.mass, 50, 600, 1, None, "A super mutant's mass (kg).";
        mutant_height = k::SUPER_MUTANT.height, 0.3, 2.5, 0.01, None, "A super mutant's centre of mass height (m).";
        mutant_stance = k::SUPER_MUTANT.stance, 0.05, 0.8, 0.01, None, "Half a super mutant's stance (m).";
    }
}

/// The roles' groups and the ranges of their values.
const ROLE_RANGES: [(&str, f32, f32, f32, &str); 4] = [
    ("hz", 0.3, 8.0, 0.05, "spring frequency (Hz)"),
    (
        "zeta",
        0.05,
        1.5,
        0.01,
        "spring damping ratio (under 1: an overshoot)",
    ),
    ("gain", 0.0, 2.0, 0.01, "share of a kick"),
    ("limit", 0.0, 1.5, 0.01, "most it turns (rad)"),
];

impl Tuning {
    /// Every value's description: the scalars, then each role's four (`Role.hz` and so on).
    pub fn fields() -> Vec<Field> {
        let mut out = Self::scalars();
        for role in f::ROLES {
            for (key, min, max, step, doc) in ROLE_RANGES {
                out.push(Field {
                    name: format!("{}.{key}", role.name()),
                    group: "Bone springs",
                    min,
                    max,
                    step,
                    ini: None,
                    doc,
                });
            }
        }
        out
    }

    fn role_slot(&mut self, name: &str) -> Option<&mut f32> {
        let (role, key) = name.split_once('.')?;
        let i = f::ROLES.iter().position(|r| r.name() == role)?;
        let r = &mut self.roles[i];
        Some(match key {
            "hz" => &mut r.hz,
            "zeta" => &mut r.zeta,
            "gain" => &mut r.gain,
            "limit" => &mut r.limit,
            _ => return None,
        })
    }

    /// The value named `name` (a field's name, or its ini key).
    pub fn get(&self, name: &str) -> Option<f32> {
        let name = Self::canonical(name);
        let mut copy = *self;
        self.get_scalar(&name)
            .or_else(|| copy.role_slot(&name).map(|v| *v))
    }

    /// The value named `name` set (held within its field's range); false for no such value.
    pub fn set(&mut self, name: &str, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        let name = Self::canonical(name);
        let Some(field) = Self::fields().into_iter().find(|f| f.name == name) else {
            return false;
        };
        let v = value.clamp(field.min, field.max);
        if self.set_scalar(&name, v) {
            return true;
        }
        match self.role_slot(&name) {
            Some(slot) => {
                *slot = v;
                true
            }
            None => false,
        }
    }

    /// A field's name for `name`, which may be its ini key (any case).
    fn canonical(name: &str) -> String {
        let name = name.trim();
        Self::scalars()
            .into_iter()
            .find(|f| f.ini.is_some_and(|k| k.eq_ignore_ascii_case(name)))
            .map_or_else(|| name.to_string(), |f| f.name)
    }

    /// The most catch steps, as a count.
    pub fn max_steps(&self) -> usize {
        if self.max_steps.is_finite() {
            self.max_steps.round().clamp(1.0, 16.0) as usize
        } else {
            1
        }
    }

    /// The values as ini text (module documentation).
    pub fn to_ini(&self) -> String {
        let mut game = format!("[{GAME_SECTION}]\n");
        let mut lab = format!(
            "\n; Compiled into the game (crates/ragdoll-core); the game reads no such section.\n\
             [{LAB_SECTION}]\n"
        );
        for field in Self::fields() {
            let v = self.get(&field.name).unwrap_or(0.0);
            match field.ini {
                Some(key) => game.push_str(&format!("{key}={}\n", v.round() as i64)),
                None => lab.push_str(&format!("{}={}\n", field.name, trim(v))),
            }
        }
        format!(
            "; nv-rs ragdoll lab tuning. Paste the [{GAME_SECTION}] lines into nv-rs.ini.\n{game}{lab}"
        )
    }

    /// The values of `text` (ini lines `key=value`, any section; `;` and `#` comments) set on a
    /// copy of `self`; answers it and the keys that were not understood.
    pub fn from_ini(&self, text: &str) -> (Tuning, Vec<String>) {
        let mut t = *self;
        let mut unknown = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                unknown.push(line.to_string());
                continue;
            };
            let value = value.split(';').next().unwrap_or("").trim();
            match value.parse::<f32>() {
                Ok(v) if t.set(key, v) => {}
                _ => unknown.push(key.trim().to_string()),
            }
        }
        (t, unknown)
    }
}

fn trim(v: f32) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s.into()
    }
}

static OVERRIDDEN: AtomicBool = AtomicBool::new(false);
static OVERRIDE: RwLock<Tuning> = RwLock::new(Tuning::GAME);

/// The values in force: [`Tuning::GAME`] unless [`set`] was called (module documentation).
pub fn get() -> Tuning {
    if !OVERRIDDEN.load(Ordering::Relaxed) {
        return Tuning::GAME;
    }
    *OVERRIDE.read().unwrap_or_else(|p| p.into_inner())
}

/// The values in force set (the lab's sliders). The game never calls this.
pub fn set(t: Tuning) {
    *OVERRIDE.write().unwrap_or_else(|p| p.into_inner()) = t;
    OVERRIDDEN.store(t != Tuning::GAME, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The game's values are the constants; every field reads and writes by its name and ini key;
    /// the text goes there and back.
    #[test]
    fn values_by_name_and_text() {
        let g = Tuning::GAME;
        assert_eq!(g.kick, f::KICK);
        assert_eq!(g.max_steps(), b::MAX_STEPS);
        assert_eq!(g.get("Pelvis.hz"), Some(3.2));
        assert_eq!(g.get("iBalance"), Some(100.0));
        assert_eq!(g.get("ihitforce"), Some(100.0));
        let fields = Tuning::fields();
        assert!(fields.len() > 100);
        for field in &fields {
            let v = g.get(&field.name).expect(&field.name);
            assert!(
                v >= field.min && v <= field.max,
                "{} = {v} outside {}..{}",
                field.name,
                field.min,
                field.max
            );
        }
        let mut t = g;
        assert!(t.set("kick", 20.0));
        assert!(t.set("iKnockdownThreshold", 150.0));
        assert!(t.set("UpperArm.limit", 0.5));
        assert!(
            t.set("kick", 1e9) && t.kick == 60.0,
            "held within its range"
        );
        assert!(!t.set("nothing", 1.0));
        let text = t.to_ini();
        assert!(text.contains("[nv-rs Gunplay]\n"));
        assert!(text.contains("iKnockdownThreshold=150\n"));
        assert!(text.contains("UpperArm.limit=0.5\n"));
        let (back, unknown) = g.from_ini(&text);
        assert!(unknown.is_empty(), "{unknown:?}");
        assert_eq!(back, t);
        let (_, unknown) = g.from_ini("[x]\nfoo=1\niBalance=50 ; half\nbar\n");
        assert_eq!(unknown, ["foo", "bar"]);
    }

    /// The override: the game's values until set, then the set ones, and back.
    #[test]
    fn the_override() {
        assert_eq!(get(), Tuning::GAME);
        // A value no pure function reads (the other tests run alongside).
        let mut t = Tuning::GAME;
        t.windmill_hz = 3.0;
        set(t);
        assert_eq!(get().windmill_hz, 3.0);
        set(Tuning::GAME);
        assert_eq!(get(), Tuning::GAME);
    }
}
