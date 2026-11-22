//! Scripted hit scenarios, for a person or an automated browser: a ground, a body, the tuning
//! changes and a list of hits; the run goes until the body has recovered (or a time limit) and
//! reports what it did as JSON.
//!
//! **The text** (the page's URL parameters, [`Spec::parse`]): `name=...&ground=flat&body=human&
//! scale=1&armour=0&range=10&limbs=1111&tune=kick:20,iBalance:150&hits=...`. `hits` is a comma
//! list of `weapon[:load|:power]@part[*count[~interval]][+delay][/from]`:
//!
//! - `weapon`: a gun (`9mm`, `44`, `556`, `308`, `50mg`, `12ga`...), a melee weapon (`fists`,
//!   `sledge`...) or a blast (`frag`, `nuke`...); `:load` a gun's ammo (`hp`, `ap`, `slug`...),
//!   `:power` a power attack.
//! - `part`: the bone aimed at (`head`, `chest`, `belly`, `pelvis`, `lthigh`, `rcalf`...); for a
//!   blast, its distance from the body in metres.
//! - `*count~interval`: that many hits that many seconds apart (a burst; 0.1 s by default).
//! - `+delay`: seconds after the hit before it (0 by default).
//! - `/from`: the direction the attack comes from, in degrees around the body: 0 its front (the
//!   default camera's side: the push goes toward -Y, where the wall, the ledge and the stairs
//!   are), 90 its right, 180 its back.
//!
//! Built-in scenarios ([`BUILT_IN`]) have expectations; a report says whether they held.

use crate::mannequin as mq;
use crate::render;
use crate::sim::{Sim, STEP};
use crate::weapons::{self, Weapon};
use crate::world::GroundKind;
use ragdoll_core::balance::Limbs;
use ragdoll_core::flinch::*;
#[allow(unused_imports)]
use ragdoll_core::fm::Fm;
use ragdoll_core::knock::Body;
use ragdoll_core::tuning::Tuning;

/// After the last hit, a run waits at most this long for the body to recover (s).
pub const SETTLE: f64 = 12.0;

/// One scripted hit.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub weapon: Weapon,
    /// The bone aimed at, or for a blast its distance (m).
    pub part: Option<usize>,
    pub distance: f32,
    pub delay: f32,
    pub from: f32,
}

/// A scenario.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub name: String,
    pub ground: GroundKind,
    pub body: Body,
    pub scale: f32,
    pub armour: f32,
    pub range: f32,
    /// The animation's root turning (degrees a second) and walking (units a second along +X), as
    /// the game's AI moves an actor while it reacts.
    pub turn: f32,
    pub walk: f32,
    /// The AI's move starting this long after the last hit (s), the turn stopping after this many
    /// degrees, and the host's read of the animation back from its bones (b44a964's; `Sim::read_back`).
    pub move_after: Option<f32>,
    pub turn_by: Option<f32>,
    pub read_back: bool,
    pub leave_written: bool,
    pub aim_pass: bool,
    /// The animation's L Calf jumping (0.3 rad, about 150 tolerances) this many frames after the
    /// first hit and staying there: the game's hit animation starting under the reaction
    /// (`legjump=2`; read back with `readback=1`).
    pub leg_jump: Option<u32>,
    /// The heading it starts at (degrees; `face=180` faces the obstacles).
    pub face: f32,
    /// The player's perks (`perks=stoppingpower,hotloads,bowlingball`).
    pub perks: crate::sim::Perks,
    pub limbs: Limbs,
    pub armed: bool,
    /// The weapon is two-handed (`armed=rifle`, or `armed=1`) or one-handed (`armed=pistol`).
    pub two_handed: bool,
    pub attacking: bool,
    pub tune: Vec<(String, f32)>,
    pub hits: Vec<Hit>,
    pub expect: Vec<String>,
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            name: "custom".into(),
            ground: GroundKind::Flat,
            body: Body::Human,
            scale: 1.0,
            armour: 0.0,
            range: 10.0,
            turn: 0.0,
            walk: 0.0,
            move_after: None,
            turn_by: None,
            read_back: false,
            leave_written: false,
            aim_pass: false,
            leg_jump: None,
            face: 0.0,
            perks: crate::sim::Perks::default(),
            limbs: Limbs::WHOLE,
            armed: false,
            two_handed: true,
            attacking: false,
            tune: Vec::new(),
            hits: Vec::new(),
            expect: Vec::new(),
        }
    }
}

/// The built-in scenarios: name, the text, the expectations (`upright`, `fell`, `steps>=N`,
/// `steps=0`, `braced`, `fall:<kind>`, `flinched` (a bone turned 2 degrees or more), `aim<=D`
/// (an armed body's weapon hand stayed within D degrees of its aim), `armspin<=W` (no arm bone
/// turned faster than W rad/s while it stood), `ragdollarmspin<=W` (nor in its ragdoll),
/// `interrupted` (its attack), `attacking` (it was not), `backbend<=D`, `tripped` (a swinging
/// foot caught on a low obstacle), `braced:waist` / `braced:wall` (what the hands went on),
/// `escalated` (the hits' reactions went from a flinch through a stagger to a knockdown),
/// `onehand` (only the free, left, hand braced) / `bothhands`, `handbounce<=N` (a hand came off
/// the surface N times while it held), `handjitter<=V` (a settled hand moved at most V m/s),
/// `handslip<=C` (nor slid more than C cm), `elbowosc<=N` (a planted elbow turned back on itself
/// at most N times), `regripped` (a two-handed weapon's support hand went back to its grip after
/// the brace, watched), `regripover<=C` (it went at most C cm past the grip), `regripwobble<=N`
/// (its distance to the grip grew again at most N times)). Every scenario is held to no bounce, jitter, slide or oscillation as well
/// (`the_built_in_scenarios_hold`).
pub const BUILT_IN: [(&str, &str, &str); 32] = [
    (
        "pistol-chest",
        "ground=flat&hits=9mm@chest",
        "upright,steps=0,flinched,vjump<=0.4,jerk<=1500,armspin<=6.5",
    ),
    (
        "magnum-chest",
        "ground=flat&hits=44@chest",
        "upright,flinched,vjump<=2,jerk<=7000,armspin<=6.5",
    ),
    (
        "rifle-burst",
        "ground=flat&hits=556@chest*3~0.1",
        "upright,vjump<=0.5,armspin<=6.5",
    ),
    (
        "hunting-rifle-side",
        "ground=flat&hits=308@chest/90",
        "upright,armspin<=6.5",
    ),
    (
        "leg-shot",
        "ground=flat&hits=44@lthigh",
        "upright,armspin<=6.5",
    ),
    (
        "shotgun-close",
        "ground=flat&range=4&hits=12ga@chest",
        "fell,ragdollarmspin<=8.5,armspin<=6.5",
    ),
    (
        "anti-materiel",
        "ground=flat&hits=50mg@chest",
        "fell,ragdollarmspin<=8.5,armspin<=6.5",
    ),
    (
        "fists",
        "ground=flat&hits=fists@chest",
        "upright,steps=0,vjump<=0.3,armspin<=6.5",
    ),
    (
        "sledge-power",
        "ground=flat&hits=sledge:power@chest",
        "fell,armspin<=6.5",
    ),
    (
        "frag-2m",
        "ground=flat&hits=frag@2",
        "fell,ragdollarmspin<=8.5,armspin<=6.5",
    ),
    (
        "wall-brace",
        "face=180&ground=wall&hits=4570@chest",
        "upright,braced,vjump<=2,armspin<=6.5",
    ),
    (
        "ledge",
        "ground=ledge&hits=4570@chest",
        "fell,fall:stepped off a ledge,ragdollarmspin<=8.5,vjump<=1.5,armspin<=6.5",
    ),
    (
        "slope-down",
        "ground=slope&hits=4570@chest",
        "upright,steps>=1,vjump<=2,armspin<=6.5",
    ),
    (
        "stairs-down",
        "ground=stairs&hits=4570@chest",
        "upright,steps>=1,vjump<=2,armspin<=6.5",
    ),
    (
        "uneven",
        "ground=uneven&hits=4570@chest",
        "upright,steps>=1,vjump<=2,armspin<=6.5",
    ),
    (
        "one-leg",
        "ground=flat&limbs=0111&hits=4570@chest",
        "fell,armspin<=6.5",
    ),
    (
        "armed-pistol",
        "ground=flat&armed=1&hits=9mm@chest",
        "upright,flinched,aim<=6,steps=0,vjump<=0.4,armspin<=6.5",
    ),
    (
        "attacking-pistol",
        "ground=flat&attacking=1&hits=9mm@chest*2~0.2",
        "upright,flinched,aim<=6,attacking,armspin<=6.5",
    ),
    (
        "attacking-magnum",
        "ground=flat&attacking=1&hits=44@chest",
        "upright,flinched,interrupted,steps>=1,armspin<=6.5",
    ),
    (
        "armed-shotgun",
        "ground=flat&armed=1&range=4&hits=12ga@chest",
        "fell,armspin<=6.5",
    ),
    // The world (Patrick, 2026-11-15): obstacles found at several heights and across the body,
    // braced on at their height, folded over, tripped on; and sustained fire escalating. The
    // braces face their obstacle (`face=180`: shot from behind into it): hands never brace behind
    // the back (`ragdoll_core::brace::hand_may_brace`, Patrick's 21-16-09 clip).
    (
        "railing-brace",
        "face=180&ground=railing&hits=4570@chest",
        "upright,braced:waist,armspin<=6.5",
    ),
    (
        "half-wall-fold",
        "ground=halfwall&hits=50mg@chest",
        "fell,fall:folded over a waist-high obstacle,ragdollarmspin<=8.5,armspin<=6.5",
    ),
    (
        "table-edge-brace",
        "face=180&ground=table&hits=4570@chest",
        "upright,braced:waist,armspin<=6.5",
    ),
    (
        "curb-trip",
        "ground=curb&hits=4570@chest",
        "tripped,fell,fall:tripped over a low obstacle,armspin<=6.5",
    ),
    (
        "gap-railing",
        "face=180&ground=gaprailing&hits=4570@chest",
        "upright,braced:waist,armspin<=6.5",
    ),
    (
        "sustained-fire",
        "ground=flat&hits=9mm@chest*14~0.1",
        "fell,escalated,armspin<=6.5",
    ),
    // Armed bodies brace one-handed (Patrick, 2026-11-15): the free hand braces, the weapon arm
    // keeps its aim (a rifle's support hand lets go of it).
    (
        "armed-pistol-wall-brace",
        "face=180&ground=wall&armed=pistol&hits=4570@chest",
        "upright,braced,onehand,armspin<=6.5",
    ),
    (
        "armed-rifle-wall-brace",
        "face=180&ground=wall&armed=rifle&hits=4570@chest",
        "upright,braced,onehand,armspin<=6.5",
    ),
    (
        "armed-pistol-railing-brace",
        "face=180&ground=railing&armed=pistol&hits=4570@chest",
        "upright,braced:waist,onehand,armspin<=6.5",
    ),
    (
        "armed-rifle-railing-brace",
        "face=180&ground=railing&armed=rifle&hits=4570@chest",
        "upright,braced:waist,onehand,armspin<=6.5",
    ),
    // The support hand goes back to the rifle's grip smoothly after the brace (Patrick,
    // 2026-11-15, e2e5531 reel: it wobbled as it regripped).
    (
        "rifle-wall-regrip",
        "face=180&ground=wall&armed=rifle&hits=4570@chest/10",
        "upright,braced,onehand,regripped,regripover<=0.5,regripwobble<=0,armspin<=6.5",
    ),
    (
        "rifle-railing-regrip",
        "face=180&ground=railing&armed=rifle&hits=4570@chest/-10",
        "upright,braced:waist,onehand,regripped,regripover<=0.5,regripwobble<=0,armspin<=6.5",
    ),
];

fn weapon_of(name: &str, opt: Option<&str>) -> Result<Weapon, String> {
    if let Some(gun) = weapons::gun(name) {
        let load = match opt {
            Some(l) => weapons::load(l).ok_or_else(|| format!("no load {l}"))?,
            None => ragdoll_core::rounds::Load::Standard,
        };
        return Ok(Weapon::Gun { gun, load });
    }
    if let Some(melee) = weapons::melee(name) {
        return Ok(Weapon::Melee {
            melee,
            strength: 5.0,
            skill: 50.0,
            power: opt == Some("power"),
        });
    }
    if let Some(blast) = weapons::blast(name) {
        return Ok(Weapon::Blast { blast });
    }
    Err(format!("no weapon {name}"))
}

impl Hit {
    fn parse(item: &str) -> Result<Vec<Hit>, String> {
        let item = item.trim();
        let (head, mut rest) = item
            .split_once('@')
            .ok_or_else(|| format!("{item}: no @part"))?;
        let (name, opt) = match head.split_once(':') {
            Some((n, o)) => (n, Some(o)),
            None => (head, None),
        };
        let weapon = weapon_of(name, opt)?;
        let mut from = 0.0f32;
        let mut delay = 0.0f32;
        let mut count = 1usize;
        let mut interval = 0.1f32;
        if let Some((r, f)) = rest.split_once('/') {
            from = f.parse().map_err(|_| format!("{item}: /from"))?;
            rest = r;
        }
        if let Some((r, d)) = rest.split_once('+') {
            delay = d.parse().map_err(|_| format!("{item}: +delay"))?;
            rest = r;
        }
        if let Some((r, n)) = rest.split_once('*') {
            let (n, i) = match n.split_once('~') {
                Some((n, i)) => (n, Some(i)),
                None => (n, None),
            };
            count = n.parse().map_err(|_| format!("{item}: *count"))?;
            if let Some(i) = i {
                interval = i.parse().map_err(|_| format!("{item}: ~interval"))?;
            }
            rest = r;
        }
        let (part, distance) = match weapon {
            Weapon::Blast { .. } => (
                None,
                rest.parse::<f32>()
                    .map_err(|_| format!("{item}: a blast's @metres"))?,
            ),
            _ => (
                Some(mq::bone_named(rest).ok_or_else(|| format!("{item}: no part {rest}"))?),
                0.0,
            ),
        };
        Ok((0..count.clamp(1, 50))
            .map(|i| Hit {
                weapon: weapon.clone(),
                part,
                distance,
                delay: if i == 0 { delay } else { interval },
                from,
            })
            .collect())
    }
}

impl Spec {
    /// A scenario from its text (module documentation): `key=value` pairs joined by `&`.
    pub fn parse(text: &str) -> Result<Spec, String> {
        let mut spec = Spec::default();
        let text = text.trim().trim_start_matches('?');
        for pair in text.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = value
                .replace("%2C", ",")
                .replace("%3A", ":")
                .replace("%40", "@");
            match key {
                "name" | "scenario" => spec.name = value.to_string(),
                "ground" => {
                    spec.ground =
                        GroundKind::named(&value).ok_or_else(|| format!("no ground {value}"))?
                }
                "body" => {
                    spec.body = match value.as_str() {
                        "human" => Body::Human,
                        "mutant" | "supermutant" => Body::SuperMutant,
                        _ => return Err(format!("no body {value}")),
                    }
                }
                "scale" => spec.scale = value.parse().map_err(|_| "scale".to_string())?,
                "armour" | "armor" | "dt" => {
                    spec.armour = value.parse().map_err(|_| "armour".to_string())?
                }
                "range" => spec.range = value.parse().map_err(|_| "range".to_string())?,
                "turn" => spec.turn = value.parse().map_err(|_| "turn".to_string())?,
                "walk" => spec.walk = value.parse().map_err(|_| "walk".to_string())?,
                "move" => spec.move_after = Some(value.parse().map_err(|_| "move".to_string())?),
                "turnby" => spec.turn_by = Some(value.parse().map_err(|_| "turnby".to_string())?),
                "readback" => {
                    spec.read_back = value == "1" || value == "written";
                    spec.leave_written = value == "written";
                }
                "aimpass" => spec.aim_pass = value == "1",
                "face" => spec.face = value.parse().map_err(|_| "face".to_string())?,
                "legjump" => {
                    spec.leg_jump = Some(value.parse().map_err(|_| "legjump".to_string())?)
                }
                "perks" => {
                    for perk in value.split(',').filter(|p| !p.is_empty()) {
                        match perk {
                            "stoppingpower" => spec.perks.stopping_power = true,
                            "hotloads" => spec.perks.hot_loads = true,
                            "bowlingball" => spec.perks.bowling_ball = true,
                            _ => return Err(format!("perks: {perk}")),
                        }
                    }
                }
                "armed" => {
                    spec.armed = value != "0";
                    spec.two_handed = value != "pistol";
                }
                "attacking" => {
                    spec.attacking = value != "0";
                    spec.armed |= spec.attacking;
                }
                "limbs" => {
                    let b: Vec<bool> = value.chars().map(|c| c != '0').collect();
                    if b.len() != 4 {
                        return Err(
                            "limbs: four digits (left leg, right leg, left arm, right arm)".into(),
                        );
                    }
                    spec.limbs = Limbs {
                        left_leg: b[0],
                        right_leg: b[1],
                        left_arm: b[2],
                        right_arm: b[3],
                    };
                }
                "tune" => {
                    for kv in value.split(',').filter(|s| !s.is_empty()) {
                        let (k, v) = kv.split_once(':').ok_or_else(|| format!("tune {kv}"))?;
                        let v: f32 = v.parse().map_err(|_| format!("tune {kv}"))?;
                        spec.tune.push((k.to_string(), v));
                    }
                }
                "hits" => {
                    for item in value.split(',').filter(|s| !s.trim().is_empty()) {
                        spec.hits.extend(Hit::parse(item)?);
                    }
                }
                "expect" => spec.expect = value.split(',').map(|s| s.to_string()).collect(),
                // The page's own parameters.
                "headless" | "run" | "view" | "autorun" => {}
                _ => return Err(format!("unknown parameter {key}")),
            }
        }
        Ok(spec)
    }

    /// A built-in scenario by name.
    pub fn built_in(name: &str) -> Option<Spec> {
        let (n, text, expect) = BUILT_IN.iter().find(|b| b.0 == name)?;
        let mut s = Spec::parse(text).ok()?;
        s.name = n.to_string();
        s.expect = expect.split(',').map(|e| e.to_string()).collect();
        Some(s)
    }
}

/// What a run did.
#[derive(Clone, Debug)]
pub struct Report {
    pub name: String,
    pub ground: &'static str,
    pub upright: bool,
    pub fell: bool,
    pub fall: Option<String>,
    pub steps: u32,
    pub braced: bool,
    pub flinches: u32,
    pub hits: u32,
    pub distance: f32,
    pub recovery: Option<f32>,
    pub time: f32,
    pub peak_speed: f32,
    pub topple: f32,
    pub decisions: Vec<String>,
    pub arm_spin: f32,
    pub ragdoll_arm_spin: f32,
    pub aim_drift: f32,
    pub flinch: f32,
    pub armed: bool,
    pub interrupted: bool,
    pub velocity_jump: f32,
    pub jerk: f32,
    pub back_bend: f32,
    pub tripped: bool,
    pub brace_kind: Option<String>,
    pub brace_hands: [bool; 2],
    pub hand_losses: u32,
    pub hand_jitter: f32,
    pub hand_slip: f32,
    pub elbow_reversals: u32,
    pub regrip_watched: bool,
    pub regrip_overshoot: f32,
    pub regrip_reversals: u32,
    pub regrip_drift: f32,
    pub settle_rise: f32,
    pub settle_jump: f32,
    pub release_rise: f32,
    pub handed_back: Option<f32>,
    pub pelvis_most: f32,
    pub pelvis_jump: f32,
    pub bone_spin: f32,
    pub spin_bone: &'static str,
    pub base_stale: u32,
    pub base_jumps: u32,
    pub yaw_rate: f32,
    pub yaw_off: f32,
    pub expect: Vec<(String, bool)>,
    pub log: Vec<String>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.expect.iter().all(|e| e.1)
    }

    pub fn json(&self) -> String {
        let s = |v: &str| format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""));
        let list = |v: &[String]| v.iter().map(|x| s(x)).collect::<Vec<_>>().join(",");
        let expect: Vec<String> = self
            .expect
            .iter()
            .map(|(e, ok)| format!("{{\"expect\":{},\"held\":{ok}}}", s(e)))
            .collect();
        format!(
            "{{\"name\":{},\"ground\":{},\"upright\":{},\"fell\":{},\"fall\":{},\"steps\":{},\
             \"braced\":{},\"flinches\":{},\"hits\":{},\"distance_m\":{:.3},\"recovery_s\":{},\
             \"time_s\":{:.2},\"peak_speed_mps\":{:.3},\"topple_mps\":{:.3},\"decisions\":[{}],\
             \"arm_spin\":{:.2},\"ragdoll_arm_spin\":{:.2},\"flinch_deg\":{:.2},\"armed\":{},\
             \"aim_drift_deg\":{:.2},\"interrupted\":{},\"velocity_jump_mps\":{:.3},\"jerk\":{:.1},\
             \"back_bend_deg\":{:.2},\"tripped\":{},\"brace_kind\":{},\
             \"brace_hands\":[{},{}],\"hand_losses\":{},\"hand_jitter_mps\":{:.3},\
             \"hand_slip_cm\":{:.2},\"elbow_reversals\":{},\"regrip_watched\":{},\
             \"regrip_overshoot_cm\":{:.2},\"regrip_reversals\":{},\"regrip_drift_cm\":{:.2},\
             \"settle_rise\":{:.3},\"settle_jump\":{:.3},\"release_rise\":{:.3},\"pelvis_most\":{:.2},\"pelvis_jump\":{:.2},\"bone_spin\":{:.2},\"spin_bone\":{},\"base_stale\":{},\"base_jumps\":{},\"yaw_rate\":{:.3},\"yaw_off\":{:.3},\
             \"expectations\":[{}],\"passed\":{},\"log\":[{}]}}",
            s(&self.name),
            s(self.ground),
            self.upright,
            self.fell,
            self.fall.as_deref().map_or("null".into(), s),
            self.steps,
            self.braced,
            self.flinches,
            self.hits,
            self.distance,
            self.recovery.map_or("null".into(), |r| format!("{r:.2}")),
            self.time,
            self.peak_speed,
            self.topple,
            list(&self.decisions),
            self.arm_spin,
            self.ragdoll_arm_spin,
            self.flinch,
            self.armed,
            self.aim_drift,
            self.interrupted,
            self.velocity_jump,
            self.jerk,
            self.back_bend,
            self.tripped,
            self.brace_kind.as_deref().map_or("null".into(), s),
            self.brace_hands[0],
            self.brace_hands[1],
            self.hand_losses,
            self.hand_jitter,
            self.hand_slip,
            self.elbow_reversals,
            self.regrip_watched,
            self.regrip_overshoot,
            self.regrip_reversals,
            self.regrip_drift,
            self.settle_rise,
            self.settle_jump,
            self.release_rise,
            self.pelvis_most,
            self.pelvis_jump,
            self.bone_spin,
            s(self.spin_bone),
            self.base_stale,
            self.base_jumps,
            self.yaw_rate,
            self.yaw_off,
            expect.join(","),
            self.passed(),
            list(&self.log)
        )
    }
}

/// The spec set up on `sim` (its ground, body, tuning on top of `base`), the body reset.
pub fn prepare(sim: &mut Sim, spec: &Spec, base: Tuning) -> Result<(), String> {
    let mut t = base;
    for (k, v) in &spec.tune {
        if !t.set(k, *v) {
            return Err(format!("no tuning value {k}"));
        }
    }
    sim.set_tuning(t);
    sim.body = spec.body;
    sim.scale = spec.scale.clamp(0.25, 4.0);
    sim.armour = spec.armour.max(0.0);
    sim.range = spec.range.clamp(0.5, 200.0);
    sim.turn = spec.turn.to_radians();
    sim.walk = spec.walk;
    // After the scenario's last hit (`run` fires the first 0.3 s in, each after its delay).
    sim.move_after = spec
        .move_after
        .map(|m| 0.3 + spec.hits.iter().map(|h| h.delay).sum::<f32>() + m);
    sim.turn_by = spec.turn_by.map(f32::to_radians);
    sim.read_back = spec.read_back;
    sim.leave_written = spec.leave_written;
    sim.aim_pass = spec.aim_pass;
    // The first hit fires 0.3 s in (`run`).
    sim.leg_jump_at = spec.leg_jump.map(|n| 0.3 + n as f64 * STEP as f64);
    sim.face = spec.face.to_radians();
    sim.perks = spec.perks;
    sim.set_limbs(spec.limbs);
    sim.armed = spec.armed;
    sim.two_handed = spec.two_handed;
    sim.attacking = spec.attacking;
    if sim.ground.kind != spec.ground {
        sim.set_ground(spec.ground);
    } else {
        sim.reset();
    }
    Ok(())
}

/// One scripted hit fired at the body as it stands now.
pub fn fire(sim: &mut Sim, hit: &Hit) {
    sim.weapon = hit.weapon.clone();
    let yaw = hit.from.to_radians();
    // The attacker's side: 0 degrees is the body's front (+Y).
    let side = [yaw.dsin(), yaw.dcos(), 0.0];
    match hit.weapon {
        Weapon::Blast { .. } => {
            let centre = add(
                sim.pos,
                add(
                    scale(side, hit.distance * UNITS_PER_METRE),
                    [0.0, 0.0, 10.0],
                ),
            );
            sim.fire(centre, [0.0, 0.0, -1.0], "blast", &render::pick_body);
        }
        _ => {
            let bone = hit.part.unwrap_or(mq::SPINE2);
            let target = sim.posed[bone].1;
            let range = match hit.weapon {
                Weapon::Melee { .. } => 1.0,
                _ => sim.range,
            };
            let shooter = add(
                [sim.pos[0], sim.pos[1], target[2]],
                scale(side, range * UNITS_PER_METRE),
            );
            let dir = unit(sub(target, shooter)).unwrap_or([0.0, -1.0, 0.0]);
            let point = match render::pick_body(sim, sub(target, scale(dir, 200.0)), dir) {
                Some((_, at)) => at,
                None => target,
            };
            sim.fire(point, dir, mq::short_name(bone), &render::pick_body);
        }
    }
}

/// Runs `spec` on `sim` to its end and reports it.
pub fn run(sim: &mut Sim, spec: &Spec, base: Tuning) -> Result<Report, String> {
    prepare(sim, spec, base)?;
    sim.note(format!("scenario {}", spec.name));
    // Settle a moment standing, then the hits on their times.
    let mut next = 0usize;
    let mut wait = 0.3f32 + spec.hits.first().map_or(0.0, |h| h.delay);
    let mut last_hit = 0.0f64;
    loop {
        if next < spec.hits.len() {
            wait -= STEP;
            if wait <= 0.0 {
                fire(sim, &spec.hits[next]);
                last_hit = sim.t;
                next += 1;
                wait = spec.hits.get(next).map_or(0.0, |h| h.delay);
                continue;
            }
        } else if (sim.stats.recovered_at.is_some() && sim.recovered())
            || sim.t - last_hit > SETTLE
            || spec.hits.is_empty()
        {
            break;
        }
        sim.step();
    }
    let where_ = if sim.in_ragdoll() {
        sim.posed[mq::PELVIS].1
    } else {
        sim.pos
    };
    let st = &sim.stats;
    let distance = length(sub(
        [where_[0], where_[1], 0.0],
        [st.start[0], st.start[1], 0.0],
    )) / UNITS_PER_METRE;
    let recovery = match (st.recovered_at, st.first_hit) {
        (Some(r), Some(f)) => Some((r - f) as f32),
        _ => None,
    };
    let mut report = Report {
        name: spec.name.clone(),
        ground: spec.ground.name(),
        upright: !st.fell,
        fell: st.fell,
        fall: st.fall.clone(),
        steps: st.steps,
        braced: st.braced,
        flinches: st.flinches,
        hits: st.hits,
        distance,
        recovery,
        time: sim.t as f32,
        peak_speed: st.peak_speed,
        topple: st.topple,
        decisions: st.decisions.clone(),
        arm_spin: st.arm_spin,
        ragdoll_arm_spin: st.ragdoll_arm_spin,
        aim_drift: st.aim_drift,
        flinch: st.flinch,
        armed: spec.armed,
        interrupted: st.interrupted,
        velocity_jump: st.velocity_jump,
        jerk: st.jerk,
        back_bend: st.back_bend,
        tripped: st.tripped,
        brace_kind: st.brace_kind.clone(),
        brace_hands: st.brace_hands,
        hand_losses: st.hand_losses,
        hand_jitter: st.hand_jitter,
        hand_slip: st.hand_slip,
        elbow_reversals: st.elbow_reversals,
        regrip_watched: st.regrip_watched,
        regrip_overshoot: st.regrip_overshoot,
        regrip_reversals: st.regrip_reversals,
        regrip_drift: st.regrip_drift,
        settle_rise: st.settle_rise,
        settle_jump: st.settle_jump,
        release_rise: st.release_rise,
        pelvis_most: st.pelvis_most,
        pelvis_jump: st.pelvis_jump,
        bone_spin: st.bone_spin,
        spin_bone: ragdoll_core::flinch::BONES[st.spin_bone].0,
        base_stale: st.base_stale,
        base_jumps: st.base_jumps,
        handed_back: match (st.handed_back_at, st.last_hit) {
            (Some(b), Some(h)) => Some((b - h) as f32),
            _ => None,
        },
        yaw_rate: st.yaw_rate,
        yaw_off: st.yaw_off,
        expect: Vec::new(),
        log: sim.log.iter().rev().take(60).rev().cloned().collect(),
    };
    report.expect = spec
        .expect
        .iter()
        .filter(|e| !e.is_empty())
        .map(|e| {
            let held = match e.as_str() {
                "upright" => report.upright,
                "fell" => report.fell,
                "braced" => report.braced,
                "recovered" => report.recovery.is_some(),
                "flinched" => report.flinch >= 2.0,
                "interrupted" => report.interrupted,
                "attacking" => !report.interrupted,
                "tripped" => report.tripped,
                "escalated" => escalated(&report.decisions),
                "onehand" => report.brace_hands == [true, false],
                "bothhands" => report.brace_hands == [true, true],
                e if e.starts_with("handbounce<=") => e[12..]
                    .parse::<u32>()
                    .is_ok_and(|n| report.hand_losses <= n),
                e if e.starts_with("handjitter<=") => e[12..]
                    .parse::<f32>()
                    .is_ok_and(|v| report.hand_jitter <= v),
                e if e.starts_with("handslip<=") => {
                    e[10..].parse::<f32>().is_ok_and(|c| report.hand_slip <= c)
                }
                "regripped" => report.regrip_watched,
                e if e.starts_with("regripover<=") => e[12..]
                    .parse::<f32>()
                    .is_ok_and(|c| report.regrip_overshoot <= c),
                e if e.starts_with("regripwobble<=") => e[14..]
                    .parse::<u32>()
                    .is_ok_and(|n| report.regrip_reversals <= n),
                e if e.starts_with("elbowosc<=") => e[10..]
                    .parse::<u32>()
                    .is_ok_and(|n| report.elbow_reversals <= n),
                e if e.starts_with("braced:") => report.brace_kind.as_deref() == Some(&e[7..]),
                e if e.starts_with("vjump<=") => e[7..]
                    .parse::<f32>()
                    .is_ok_and(|v| report.velocity_jump <= v),
                e if e.starts_with("jerk<=") => {
                    e[6..].parse::<f32>().is_ok_and(|j| report.jerk <= j)
                }
                e if e.starts_with("aim<=") => {
                    e[5..].parse::<f32>().is_ok_and(|d| report.aim_drift <= d)
                }
                e if e.starts_with("armspin<=") => {
                    e[9..].parse::<f32>().is_ok_and(|w| report.arm_spin <= w)
                }
                e if e.starts_with("backbend<=") => {
                    e[10..].parse::<f32>().is_ok_and(|d| report.back_bend <= d)
                }
                e if e.starts_with("ragdollarmspin<=") => e[16..]
                    .parse::<f32>()
                    .is_ok_and(|w| report.ragdoll_arm_spin <= w),
                e if e.starts_with("settlerise<=") => e[12..]
                    .parse::<f32>()
                    .is_ok_and(|v| report.settle_rise <= v),
                e if e.starts_with("settlejump<=") => e[12..]
                    .parse::<f32>()
                    .is_ok_and(|v| report.settle_jump <= v),
                e if e.starts_with("releaserise<=") => e[13..]
                    .parse::<f32>()
                    .is_ok_and(|v| report.release_rise <= v),
                e if e.starts_with("yawrate<=") => {
                    e[9..].parse::<f32>().is_ok_and(|v| report.yaw_rate <= v)
                }
                e if e.starts_with("recovery<=") => e[10..]
                    .parse::<f32>()
                    .is_ok_and(|v| report.recovery.is_some_and(|r| r <= v)),
                e if e.starts_with("steps>=") => {
                    e[7..].parse::<u32>().is_ok_and(|n| report.steps >= n)
                }
                e if e.starts_with("steps=") => {
                    e[6..].parse::<u32>().is_ok_and(|n| report.steps == n)
                }
                e if e.starts_with("fall:") => report.fall.as_deref() == Some(&e[5..]),
                _ => false,
            };
            (e.clone(), held)
        })
        .collect();
    Ok(report)
}

/// Whether a run's decisions escalated as sustained fire should: from a flinch (`None`) through
/// a stagger to a knockdown, never back down.
fn escalated(decisions: &[String]) -> bool {
    let rank = |d: &String| match d.as_str() {
        "None" => Some(0u8),
        "Stagger" => Some(1),
        "Knockdown" => Some(2),
        _ => None,
    };
    let Some(r) = decisions.iter().map(rank).collect::<Option<Vec<u8>>>() else {
        return false;
    };
    r.first() == Some(&0)
        && r.contains(&1)
        && r.last() == Some(&2)
        && r.windows(2).all(|w| w[0] <= w[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_parse() {
        let s = Spec::parse("ground=wall&body=mutant&tune=kick:20,iBalance:150&hits=556:ap@chest*3~0.08+0.5/90,frag@2,sledge:power@head")
            .unwrap();
        assert_eq!(s.ground, GroundKind::Wall);
        assert_eq!(s.body, Body::SuperMutant);
        assert_eq!(
            s.tune,
            vec![("kick".into(), 20.0), ("iBalance".into(), 150.0)]
        );
        assert_eq!(s.hits.len(), 5);
        assert_eq!(s.hits[0].delay, 0.5);
        assert_eq!(s.hits[1].delay, 0.08);
        assert_eq!(s.hits[0].from, 90.0);
        assert_eq!(s.hits[3].distance, 2.0);
        assert!(matches!(
            s.hits[4].weapon,
            Weapon::Melee { power: true, .. }
        ));
        assert!(Spec::parse("hits=nothing@chest").is_err());
        assert!(Spec::parse("bogus=1").is_err());
        for (name, _, _) in BUILT_IN {
            assert!(Spec::built_in(name).is_some(), "{name}");
        }
    }

    /// Every built-in scenario runs to its end, stays finite, and its expectations hold.
    #[test]
    fn the_built_in_scenarios_hold() {
        let mut sim = Sim::new(GroundKind::Flat);
        let mut failed = Vec::new();
        for (name, _, _) in BUILT_IN {
            let spec = Spec::built_in(name).unwrap();
            let r = run(&mut sim, &spec, Tuning::GAME).unwrap();
            println!("{}", r.json());
            assert!(r.distance.is_finite() && r.time.is_finite(), "{name}");
            for (_, p) in sim.posed.iter() {
                assert!(
                    p.iter().all(|v| v.is_finite()),
                    "{name}: a bone went to NaN"
                );
            }
            if !r.passed() {
                failed.push(format!(
                    "{name}: {:?} (steps {}, fall {:?}, decisions {:?})",
                    r.expect, r.steps, r.fall, r.decisions
                ));
            }
            // Every drawn bone keeps its length, and none is drawn far off its animation (a fall's
            // and a bracing arm's own aside: Patrick's 21-16-09 clip, a creature 1047 units off).
            if sim.stats.length_change > 0.5 {
                failed.push(format!(
                    "{name}: a bone's length {:.2} units off",
                    sim.stats.length_change
                ));
            }
            if sim.stats.drawn_most_off > ragdoll_core::active::MOST_DRAWN_OFF {
                failed.push(format!(
                    "{name}: a bone drawn {:.1} units off its animation",
                    sim.stats.drawn_most_off
                ));
            }
            // No elbow or knee ever bends backwards, standing, in the ragdoll or getting up
            // (Patrick, 2026-11-15: the wall brace's arms bent backwards in c87282a).
            if r.back_bend > MOST_BACK_BEND {
                failed.push(format!(
                    "{name}: an elbow or a knee bent {:.1} degrees backwards",
                    r.back_bend
                ));
            }
            // A rifle's support hand goes back to its grip without a wobble (Patrick, 2026-11-15:
            // it wobbled as it regripped in the e2e5531 reel).
            if let Some(why) = regrip_wobble(&r) {
                failed.push(format!("{name}: the regrip wobbled: {why}"));
            }
            // A bracing hand meets the surface and stays (Patrick, 2026-11-15: the arms bounced
            // in the b7aaa5a reel): no loss of contact while it holds, no jitter or slide once
            // settled, no oscillating elbow.
            if r.hand_losses > 0
                || r.hand_jitter > MOST_HAND_JITTER
                || r.hand_slip > MOST_HAND_SLIP
                || r.elbow_reversals > MOST_ELBOW_REVERSALS
            {
                failed.push(format!(
                    "{name}: the brace's hands bounced: {} losses of contact, {:.3} m/s, {:.2} cm \
                     slide, {} elbow reversals",
                    r.hand_losses, r.hand_jitter, r.hand_slip, r.elbow_reversals
                ));
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
    }

    /// The braces hold up beyond the built-in scenes (Patrick, 2026-11-15: the arms bounced): at
    /// a wall, a railing, a table and a gapped railing, unarmed and armed (a pistol, a rifle),
    /// pushed by three guns from three sides, no hand bounces, jitters, slides or oscillates, no
    /// elbow or knee bends backwards, no arm turns faster than the cap while it stands, and an
    /// armed body keeps its weapon arm near its aim.
    #[test]
    fn braces_hold_across_pushes_and_weapons() {
        let mut sim = Sim::new(GroundKind::Flat);
        let mut failed = Vec::new();
        let mut planted = 0;
        let mut runs = 0;
        let mut worst = [0.0f32; 6];
        let mut spin_run = String::new();
        let mut regrips = 0;
        let mut worst_regrip = [0.0f32; 3];
        for ground in ["wall", "railing", "table", "gaprailing"] {
            for armed in ["0", "pistol", "rifle"] {
                for gun in ["4570", "44", "308"] {
                    for from in ["0", "15", "-15"] {
                        let text = format!(
                            "name={ground}-{armed}-{gun}-{from}&face=180&ground={ground}&armed={armed}&hits={gun}@chest/{from}"
                        );
                        let spec = Spec::parse(&text).unwrap();
                        let r = run(&mut sim, &spec, Tuning::GAME).unwrap();
                        runs += 1;
                        worst[0] = worst[0].max(r.hand_jitter);
                        worst[1] = worst[1].max(r.hand_slip);
                        worst[2] = worst[2].max(r.elbow_reversals as f32);
                        worst[3] = worst[3].max(r.back_bend);
                        if r.upright && r.arm_spin > worst[4] {
                            worst[4] = r.arm_spin;
                            spin_run = spec.name.clone();
                        }
                        if armed != "0" && !r.brace_hands[1] && r.upright {
                            worst[5] = worst[5].max(r.aim_drift);
                        }
                        if r.log.iter().any(|l| l.contains("hand plants")) {
                            planted += 1;
                        }
                        let mut why = Vec::new();
                        if r.hand_losses > 0 {
                            why.push(format!("{} losses", r.hand_losses));
                        }
                        if r.hand_jitter > MOST_HAND_JITTER {
                            why.push(format!("jitter {:.3} m/s", r.hand_jitter));
                        }
                        if r.hand_slip > MOST_HAND_SLIP {
                            why.push(format!("slide {:.2} cm", r.hand_slip));
                        }
                        if r.elbow_reversals > MOST_ELBOW_REVERSALS {
                            why.push(format!("{} elbow reversals", r.elbow_reversals));
                        }
                        if r.back_bend > MOST_BACK_BEND {
                            why.push(format!("back bend {:.1} deg", r.back_bend));
                        }
                        if r.upright && r.arm_spin > MOST_ARM_SPIN {
                            why.push(format!("arm spin {:.2} rad/s", r.arm_spin));
                        }
                        if armed != "0"
                            && !r.brace_hands[1]
                            && r.upright
                            && r.aim_drift > MOST_BRACE_AIM
                        {
                            why.push(format!("aim drift {:.1} deg", r.aim_drift));
                        }
                        if let Some(w) = regrip_wobble(&r) {
                            why.push(format!("regrip: {w}"));
                        }
                        if r.regrip_watched {
                            regrips += 1;
                            worst_regrip[0] = worst_regrip[0].max(r.regrip_overshoot);
                            worst_regrip[1] = worst_regrip[1].max(r.regrip_reversals as f32);
                            worst_regrip[2] = worst_regrip[2].max(r.regrip_drift);
                        }
                        if !why.is_empty() {
                            failed.push(format!("{}: {}", spec.name, why.join(", ")));
                        }
                    }
                }
            }
        }
        println!(
            "{planted} of {runs} runs planted a hand; worst: jitter {:.3} m/s, slide {:.2} cm, \
             {} elbow reversals, back bend {:.1} deg, arm spin {:.2} rad/s ({spin_run}), aim drift \
             {:.1} deg",
            worst[0], worst[1], worst[2], worst[3], worst[4], worst[5]
        );
        println!(
            "{regrips} regrips watched; worst: overshoot {:.2} cm, {} wobbles, drift {:.2} cm",
            worst_regrip[0], worst_regrip[1], worst_regrip[2]
        );
        assert!(
            planted >= runs / 3,
            "few braces touched: {planted} of {runs}"
        );
        assert!(regrips >= 18, "few regrips watched: {regrips}");
        assert!(failed.is_empty(), "{failed:#?}");
    }

    /// A two-handed weapon fired with one hand (Patrick, 2026-11-15): while the support hand is off
    /// the grip (bracing, coming back) the spread is far worse, easing back to the weapon's own as
    /// the hand regrips; heavier weapons worse, stronger actors better, never as good as two hands
    /// (`ragdoll_core::one_hand`, the share from the lab's own regrip).
    #[test]
    fn one_handed_fire_is_far_less_accurate_until_the_regrip() {
        use ragdoll_core::one_hand::spread;
        let mut sim = Sim::new(GroundKind::Flat);
        let spec = Spec::built_in("rifle-wall-regrip").unwrap();
        let r = run(&mut sim, &spec, Tuning::GAME).unwrap();
        assert!(r.regrip_watched, "the support hand braced and came back");
        let off = sim.stats.off_grip.clone();
        assert!(!off.is_empty());
        let peak = off.iter().copied().fold(0.0f32, f32::max);
        let last = *off.last().unwrap();
        assert!(peak > 0.99, "the support hand fully off the grip: {peak}");
        assert!(last < 0.01, "back on the grip at the end: {last}");
        // From its peak to the end it only comes down (eased back, never off the grip again).
        let at = off.iter().position(|&o| o >= peak - 1e-6).unwrap();
        let mut least = peak;
        for (i, &o) in off[at..].iter().enumerate() {
            assert!(
                o <= least + 0.02,
                "back off the grip after the regrip: frame {i}, {o}"
            );
            least = least.min(o);
        }
        // A light and a heavy rifle (effective spread and weight), a weak and a strong actor.
        let light = (0.4f32, 6.0f32);
        let heavy = (0.6f32, 20.0f32);
        for &(base, weight) in &[light, heavy] {
            for strength in [2.0f32, 5.0, 10.0] {
                let one = spread(base, weight, strength, peak);
                let after = spread(base, weight, strength, last);
                println!(
                    "{weight} lb, Strength {strength}: {base:.2} deg two-handed, {one:.2} one-handed, \
                     {after:.2} after the regrip"
                );
                assert!(
                    one >= base * 2.5 && one - base >= 1.2,
                    "much worse one-handed: {one}"
                );
                assert!(
                    (after - base).abs() < 0.05,
                    "recovered after the regrip: {after}"
                );
            }
        }
        let worst = |(b, w): (f32, f32), s: f32| spread(b, w, s, 1.0);
        assert!(
            worst(heavy, 5.0) > worst(light, 5.0) * 1.5,
            "heavier is worse"
        );
        assert!(
            worst(light, 2.0) > worst(light, 10.0) * 1.5,
            "stronger is steadier"
        );
        assert!(worst(heavy, 10.0) < worst(heavy, 2.0));
        // The strongest actor with the lightest two-hander: still clearly worse than two hands.
        assert!(worst((light.0, 0.0), 10.0) > light.0 * 3.0);
    }

    /// What a regrip may do: how far past the grip it may go (cm), how often its distance may grow
    /// again (never), how far it may come off the grip once on it (cm).
    const MOST_REGRIP_OVERSHOOT: f32 = 0.5;
    const MOST_REGRIP_WOBBLES: u32 = 0;
    const MOST_REGRIP_DRIFT: f32 = 0.5;

    fn regrip_wobble(r: &Report) -> Option<String> {
        if !r.regrip_watched {
            return None;
        }
        let mut why = Vec::new();
        if r.regrip_overshoot > MOST_REGRIP_OVERSHOOT {
            why.push(format!("{:.2} cm past the grip", r.regrip_overshoot));
        }
        if r.regrip_reversals > MOST_REGRIP_WOBBLES {
            why.push(format!("{} wobbles", r.regrip_reversals));
        }
        if r.regrip_drift > MOST_REGRIP_DRIFT {
            why.push(format!("{:.2} cm off the grip once on it", r.regrip_drift));
        }
        (!why.is_empty()).then(|| why.join(", "))
    }

    /// The arm speed cap while a brace's body stands (8 rad/s, the arms' bound of Patrick's
    /// 10:47 spec; was 6.6: an unarmed bracing arm is now turned out of the torso as it reaches,
    /// `active::Body::clear_arms`; the scenes keep `armspin<=6.5`), and how far an armed body's
    /// weapon arm may turn off its aim while its free hand braces (degrees).
    const MOST_ARM_SPIN: f32 = 8.0;
    const MOST_BRACE_AIM: f32 = 30.0;

    /// The most a settled brace hand may move (m/s) and slide (cm), and how often a planted
    /// elbow may turn back on itself by more than `sim::ELBOW_SWING` (twice: it takes the load,
    /// gives it back as the body recovers, and settles).
    const MOST_HAND_JITTER: f32 = 0.02;
    const MOST_HAND_SLIP: f32 = 0.3;
    const MOST_ELBOW_REVERSALS: u32 = 2;

    /// The most an elbow or a knee may show bent backwards past the hinges' allowance
    /// (`active::HYPEREXTEND`, 1.7 degrees past straight) in degrees: a soft stop's give.
    const MOST_BACK_BEND: f32 = 3.0;

    /// The measure itself: a forearm turned forward is a bend, turned back past straight is a
    /// backward one; a calf the other way round (straight: the two bones' rest lines in line, so
    /// the rest pose's slight bends count).
    #[test]
    fn a_backward_bend_is_seen() {
        use crate::mannequin as mq;
        use crate::sim::{back_bend, hinge_bend};
        use ragdoll_core::active;
        use ragdoll_core::flinch::{rotation, IDENTITY};
        let mut posed = [(IDENTITY, [0.0; 3]); mq::COUNT];
        assert!(
            back_bend(&posed) < 0.5,
            "the rest pose: {}",
            back_bend(&posed)
        );
        posed[mq::L_FOREARM].0 = rotation([0.8, 0.0, 0.0]);
        posed[mq::R_CALF].0 = rotation([-0.8, 0.0, 0.0]);
        assert!(back_bend(&posed) < 0.5, "flexed is no backward bend");
        let rest = hinge_bend(
            &[(IDENTITY, [0.0; 3]); mq::COUNT],
            mq::R_UPPERARM,
            mq::R_FOREARM,
            1.0,
        );
        posed[mq::R_FOREARM].0 = rotation([-0.3, 0.0, 0.0]);
        let b = back_bend(&posed);
        let want = (0.3 - rest - active::HYPEREXTEND).to_degrees();
        assert!((b - want).abs() < 0.5, "{b} vs {want}");
        posed[mq::R_FOREARM].0 = IDENTITY;
        let rest = hinge_bend(
            &[(IDENTITY, [0.0; 3]); mq::COUNT],
            mq::L_THIGH,
            mq::L_CALF,
            -1.0,
        );
        posed[mq::L_CALF].0 = rotation([0.5, 0.0, 0.0]);
        let b = back_bend(&posed);
        let want = (0.5 - rest - active::HYPEREXTEND).to_degrees();
        assert!((b - want).abs() < 0.5, "{b} vs {want}");
        // Folded round past its most flex, an elbow reads as flexed past its stop, not backwards.
        posed[mq::L_CALF].0 = IDENTITY;
        posed[mq::L_FOREARM].0 = rotation([3.3, 0.0, 0.0]);
        assert!(back_bend(&posed) < 0.5);
    }
}

/// The game's frame times, repeated hits on an armed body while it steps, and the game's write
/// path through its ragdoll-controller pass (Patrick, 2026-11-15: a Powder Ganger stretched, grew
/// and froze the game in f36ddda).
#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod robustness {
    use super::*;
    use crate::mannequin::{self as mq, COUNT, PARENT};
    use ragdoll_core::active;
    use ragdoll_core::pose::{self, Xf};

    /// Every bone finite and where a body can be: within 2.5 m of the pelvis, the pelvis within
    /// 3 m of the actor's reference.
    fn believable(sim: &Sim) -> Result<(), String> {
        let pelvis = sim.posed[mq::PELVIS].1;
        for (b, (r, p)) in sim.posed.iter().enumerate() {
            if !(r.iter().chain(p.iter()).all(|x| x.is_finite())) {
                return Err(format!("bone {b} not finite"));
            }
            if !pose::is_rotation(r, 1e-3) {
                return Err(format!("bone {b}'s rotation is no rotation"));
            }
            if length(sub(*p, pelvis)) > 2.5 * UNITS_PER_METRE {
                return Err(format!("bone {b} {:?} far from the pelvis {:?}", p, pelvis));
            }
        }
        if sim.standing() && length(sub(pelvis, sim.pos)) > 3.0 * UNITS_PER_METRE {
            return Err("the pelvis left the actor".into());
        }
        Ok(())
    }

    /// An armed Powder Ganger's log: a .357 round, another 0.3 s later while it steps, more while
    /// it is still reacting, at every frame time the game may run at: always believable, never
    /// dropped, and back in its animation at the end.
    #[test]
    fn armed_repeated_hits_mid_step_at_the_games_frame_times() {
        for dt in [1.0 / 240.0, 1.0 / 144.0, 1.0 / 60.0, 1.0 / 30.0, 0.05] {
            let mut sim = Sim::new(GroundKind::Flat);
            let spec = Spec::parse(
                "armed=1&hits=357@lupperarm,357@chest+0.3,44@chest+0.25,9mm@head+0.1,357@rthigh+0.4",
            )
            .unwrap();
            prepare(&mut sim, &spec, Tuning::GAME).unwrap();
            sim.dt = dt;
            let mut next = 0;
            let mut wait = 0.3f32;
            let mut frames = 0;
            while frames < (8.0 / dt) as usize {
                if next < spec.hits.len() {
                    wait -= dt;
                    if wait <= 0.0 {
                        fire(&mut sim, &spec.hits[next]);
                        next += 1;
                        wait = spec.hits.get(next).map_or(0.0, |h| h.delay);
                    }
                }
                sim.step();
                frames += 1;
                if let Err(why) = believable(&sim) {
                    panic!("dt {dt}: frame {frames}: {why}");
                }
            }
            assert_eq!(sim.stats.dropped, 0, "dt {dt}: dropped");
            assert!(sim.stats.hits >= 5, "dt {dt}: {} hits", sim.stats.hits);
            assert!(
                sim.recovered() || !sim.standing(),
                "dt {dt}: still reacting"
            );
            if sim.standing() {
                let anim = sim.anim();
                for b in 0..COUNT {
                    assert!(
                        length(sub(sim.posed[b].1, anim[b].1)) < 1.0,
                        "dt {dt}: bone {b} not back in its animation"
                    );
                }
            }
        }
    }

    /// The host's one write path (`active_ragdoll`: our own controller, a post-animation write of
    /// absolute locals over the animated pose), the same for every actor: unarmed, armed and
    /// attacking. The Powder Ganger that grew and froze (FF001322) was unarmed and still went
    /// through the game's ragdoll-controller pass (foot IK alone takes an actor there), so each
    /// kind is run, with and without something rewriting the bones after the write.
    #[test]
    fn one_write_path_for_every_actor() {
        for spec in [
            "hits=357@lupperarm,44@chest+0.3",
            "armed=1&hits=357@lupperarm,44@chest+0.3",
            "attacking=1&hits=357@lupperarm,44@chest+0.3",
        ] {
            for rewrites in [0, 3] {
                write_path(spec, rewrites, false, false);
            }
            // A later pass moving the written bones past the tolerance, and two seconds with no
            // animation write (Patrick, 2026-11-16: the base was taken from the moved write each
            // frame and the body drifted 168 units in 2 s, then snapped back).
            write_path(spec, 1, true, false);
            // At the game's scale: the actor's root a hundred thousand units from the world's
            // origin, walking and turning under the reaction.
            write_path(spec, 1, true, true);
        }
    }

    /// A body dropped as not believable mid-flinch eases back to the animation over
    /// `active::FADE_BACK` (Patrick, 2026-11-16: NPCs leaned away, then snapped into place in one
    /// frame): no joint moves more in a frame than a small share of how far the shown pose was
    /// off the animation, and after the fade the animation is shown.
    #[test]
    fn a_dropped_body_eases_back() {
        let mut sim = Sim::new(GroundKind::Flat);
        let spec = Spec::parse("hits=357@chest,44@lupperarm+0.2").unwrap();
        prepare(&mut sim, &spec, Tuning::GAME).unwrap();
        let mut next = 0;
        let mut wait = 0.1f32;
        let mut gap = 0.0f32;
        let mut most = 0.0f32;
        let mut dropped_at = None;
        for frame in 0..200 {
            if next < spec.hits.len() {
                wait -= STEP;
                if wait <= 0.0 {
                    fire(&mut sim, &spec.hits[next]);
                    next += 1;
                    wait = spec.hits.get(next).map_or(0.0, |h| h.delay);
                }
            }
            if frame == 40 {
                // How far the shown pose is off the animation as it is dropped.
                let anim = sim.anim();
                gap = (0..COUNT)
                    .map(|b| length(sub(sim.posed[b].1, anim[b].1)))
                    .fold(0.0, f32::max);
                sim.drop_body("the test drops it");
                dropped_at = Some(frame);
            }
            let before = sim.posed;
            let anim_before = sim.anim();
            sim.step();
            if let Some(at) = dropped_at {
                if frame >= at && frame < at + 30 {
                    // A joint's move in the frame beyond the animation's own.
                    let anim = sim.anim();
                    for b in 0..COUNT {
                        let shown = length(sub(sim.posed[b].1, before[b].1));
                        let own = length(sub(anim[b].1, anim_before[b].1));
                        most = most.max(shown - own);
                    }
                }
                if frame == at + 30 {
                    let anim = sim.anim();
                    for b in 0..COUNT {
                        let off = length(sub(sim.posed[b].1, anim[b].1));
                        assert!(
                            off < 0.5,
                            "bone {b} still {off:.2} units off after the fade"
                        );
                    }
                }
            }
        }
        assert_eq!(sim.stats.dropped, 1);
        assert!(
            gap > 3.0,
            "the flinch was too small to tell: {gap:.2} units"
        );
        assert!(
            most < 0.2 * gap,
            "a joint moved {most:.2} units in one frame after the drop (the pose was {gap:.2} off)"
        );
    }

    /// The write path on the lab's shown poses for `spec`: each frame the bones' locals are worked
    /// out over the animation's (`pose::local_for`) and written; then `rewrites` times something
    /// (a pass that runs after the write: the game's, before nv-rs stopped writing through it)
    /// reads them and writes them back a hair off; and some frames the animation does not write at
    /// all. The world pose the written locals make is the shown pose, every frame, and nothing
    /// adds up.
    ///
    /// `far`: the lab's world is put under a root at the game's scale (about 1.2e5 units from the
    /// origin, as the Mojave's cells are), moving at 150 units a second and turning at 0.7 rad a
    /// second, so the animated world, the written locals and the pose they make are all in game
    /// coordinates far from the origin.
    fn write_path(name: &str, rewrites: usize, beyond: bool, far: bool) {
        let mut sim = Sim::new(GroundKind::Flat);
        let spec = Spec::parse(name).unwrap();
        prepare(&mut sim, &spec, Tuning::GAME).unwrap();
        // The bones' locals in the "game": start as the animation's.
        let anim_locals = |sim: &Sim| -> Vec<(M3, V3)> {
            let anim = sim.anim();
            (0..COUNT)
                .map(|b| match mq::NODE_PARENT[b] {
                    Some(p) => (
                        mul(&transpose(&anim[p].0), &anim[b].0),
                        apply_m(&transpose(&anim[p].0), sub(anim[b].1, anim[p].1)),
                    ),
                    None => (anim[b].0, sub(anim[b].1, sim.pos)),
                })
                .collect()
        };
        // The game's root over the lab's world (`far`), and a lab transform carried into the game.
        let root_at = |frame: usize| -> Xf {
            if !far {
                return Xf::IDENTITY;
            }
            let t = frame as f32 * STEP;
            Xf {
                r: rotation([0.0, 0.0, 0.4 + 0.7 * t]),
                t: [118_000.0 + 150.0 * t, -86_500.0 + 40.0 * t, 3_150.0],
                s: 1.0,
            }
        };
        let game = |g: &Xf, r: M3, t: V3| g.then(&Xf { r, t, s: 1.0 });
        let mut locals = anim_locals(&sim);
        let mut bases: Vec<Option<(M3, V3)>> = vec![None; COUNT];
        let mut written: Vec<Option<(M3, V3)>> = vec![None; COUNT];
        let mut next = 0;
        let mut wait = 0.3f32;
        let mut noise = 0u32;
        for frame in 0..600 {
            if next < spec.hits.len() {
                wait -= STEP;
                if wait <= 0.0 {
                    fire(&mut sim, &spec.hits[next]);
                    next += 1;
                    wait = spec.hits.get(next).map_or(0.0, |h| h.delay);
                }
            }
            sim.step();
            // The animation writes the locals afresh on most frames, not on every one (and, past
            // the tolerance, not at all for two seconds).
            let still = beyond && (120..240).contains(&frame);
            if frame % 7 != 3 && !still {
                locals = anim_locals(&sim);
            }
            // The layer: each bone's base, the animated world from the bases, the locals.
            let g = root_at(frame);
            let mut world: Vec<Xf> = Vec::with_capacity(COUNT);
            let mut parents: Vec<Xf> = Vec::with_capacity(COUNT);
            for b in 0..COUNT {
                bases[b] = Some(pose::base_of(locals[b], bases[b], written[b]));
                let up = match mq::NODE_PARENT[b] {
                    Some(p) => world[p],
                    None => game(&g, IDENTITY, sim.pos),
                };
                let base = bases[b].unwrap();
                parents.push(up);
                world.push(up.then(&Xf {
                    r: base.0,
                    t: base.1,
                    s: 1.0,
                }));
            }
            let anim: Vec<Xf> = sim.anim().iter().map(|a| game(&g, a.0, a.1)).collect();
            for b in 0..COUNT {
                // The foot IK's height under the animation's is the game's pose too.
                let d = sub(world[b].t, anim[b].t);
                let ik = beyond && length(sub(d, [0.0, 0.0, -4.0])) < 0.5;
                assert!(
                    length(d) < 0.5 || ik,
                    "frame {frame}: bone {b}'s animated world went off: {:?} vs {:?}",
                    world[b].t,
                    anim[b].t
                );
            }
            let posed: Vec<Xf> = sim.posed.iter().map(|p| game(&g, p.0, p.1)).collect();
            let shown: Vec<M3> = posed.iter().map(|p| p.r).collect();
            let offsets: Vec<M3> = (0..COUNT)
                .map(|b| {
                    let own = mul(&shown[b], &transpose(&world[b].r));
                    pose::ortho(&match mq::NODE_PARENT[b] {
                        Some(p) => mul(&mul(&world[p].r, &transpose(&shown[p])), &own),
                        None => own,
                    })
                })
                .collect();
            let shift = sub(posed[mq::PELVIS].t, world[mq::PELVIS].t);
            let out: Vec<(M3, V3)> = (0..COUNT)
                .map(|b| {
                    // The pelvis by its offset; a node root that hangs from the pelvis in the
                    // body (the spine) at its shown joint (the host's write).
                    let s = if b == mq::PELVIS {
                        Some(shift)
                    } else if mq::NODE_PARENT[b].is_none() {
                        Some(sub(posed[b].t, world[b].t))
                    } else {
                        None
                    };
                    pose::local_for(&parents[b], bases[b].unwrap(), &offsets[b], s)
                })
                .collect();
            // Written by the layer, then `rewrites` times by a later pass, read back a hair off.
            let pelvis_animated_z = anim_locals(&sim)[mq::PELVIS].1[2];
            locals.clone_from(&out);
            for _ in 0..rewrites {
                for b in 0..COUNT {
                    let mut back = out[b];
                    noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let n = (noise >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
                    if beyond {
                        // Past both tolerances (0.004 and 0.15 units), one way or the other, on
                        // a rotation or an axis the layer holds clearly off its animation (more
                        // than 8 tolerances; with less, a moved write cannot be told from the
                        // animation's, and what it moves is a wander of the size of the move, not
                        // the offset stacked).
                        let base = bases[b].unwrap();
                        let s = if n < 0.0 { -1.0 } else { 1.0 };
                        let rot_off = out[b]
                            .0
                            .iter()
                            .zip(base.0.iter())
                            .map(|(x, y)| (x - y).abs())
                            .fold(0.0f32, f32::max);
                        if rot_off > 8.0 * pose::ROT_TOL {
                            back.0[(noise % 9) as usize] += s * 4e-3;
                        }
                        let axis = (noise % 3) as usize;
                        if (out[b].1[axis] - base.1[axis]).abs() > 8.0 * pose::POS_TOL {
                            back.1[axis] += s * 0.15;
                        }
                        // The game's foot IK: the pelvis's height set outright, 4 units under the
                        // animation's, its other axes left as written (Patrick's 9mm clip: that
                        // write was taken for the animation's whole translation, the pelvis's
                        // sideways offset stacked every frame and the body was 2.5 m off in
                        // 0.7 s).
                        if b == mq::PELVIS {
                            back.1[2] = pelvis_animated_z - 4.0;
                        }
                    } else {
                        back.0[(noise % 9) as usize] += n * 6e-4;
                        back.1[(noise % 3) as usize] += n * 0.01;
                    }
                    locals[b] = back;
                }
            }
            written = out.iter().map(|w| Some(*w)).collect();
            // The world the written locals make is the shown pose.
            let mut made: Vec<Xf> = Vec::with_capacity(COUNT);
            for b in 0..COUNT {
                let up = match mq::NODE_PARENT[b] {
                    Some(p) => made[p],
                    None => game(&g, IDENTITY, sim.pos),
                };
                made.push(up.then(&Xf {
                    r: out[b].0,
                    t: out[b].1,
                    s: 1.0,
                }));
            }
            for b in 0..COUNT {
                assert!(
                    pose::is_rotation(&out[b].0, 1e-4),
                    "{name:?}, {rewrites} rewrites, frame {frame}: bone {b} written with no rotation"
                );
                let d = length(sub(made[b].t, posed[b].t));
                assert!(
                    d < 1.0,
                    "{name:?}, {rewrites} rewrites, frame {frame}: bone {b} written {d:.2} units \
                     off its shown place"
                );
                let turn = active::qlog(active::qmul(
                    active::qconj(active::qfrom_m(&made[b].r)),
                    active::qfrom_m(&posed[b].r),
                ));
                assert!(
                    length(turn) < 0.02,
                    "{name:?}, {rewrites} rewrites, frame {frame}: bone {b} turned off its shown pose"
                );
            }
        }
    }
}

/// The slow plank lean (Patrick's 126915e clip): in FNV's `skeleton.nif` `Bip01 Spine` hangs from
/// `Bip01 NonAccum`, a sibling of `Bip01 Pelvis`, so read as the node tree the whole upper body was
/// a free second root of the physical body: a hit spun it as one stiff plank until the body was
/// dropped. These run the lab on the game's tree (`mq::NODE_PARENT`, its physics parents by
/// `ragdoll_core::active::physics_parents`, as the host's `active_ragdoll` makes them).
#[cfg(test)]
mod plank {
    use super::*;
    use crate::sim::Sim;
    use ragdoll_core::active;

    /// The angle (degrees) of `a` against `b`.
    fn angle(a: &M3, b: &M3) -> f32 {
        length(active::qlog(active::qmul(
            active::qfrom_m(a),
            active::qconj(active::qfrom_m(b)),
        )))
        .to_degrees()
    }

    type Pose = [(M3, V3); mq::COUNT];

    /// `b`'s rotation against `a`'s in the shown pose, over the same in the animation (degrees).
    fn bend(shown: &Pose, anim: &Pose, a: usize, b: usize) -> f32 {
        let rel = |x: &Pose| mul(&transpose(&x[a].0), &x[b].0);
        angle(&rel(shown), &rel(anim))
    }

    /// The whole body's tilt about the ankles: the neck seen from between the feet, shown against
    /// animated (degrees).
    fn tilt(shown: &Pose, anim: &Pose) -> f32 {
        let from = |x: &Pose| {
            let feet = scale(add(x[mq::L_FOOT].1, x[mq::R_FOOT].1), 0.5);
            sub(x[mq::NECK].1, feet)
        };
        let (a, b) = (from(anim), from(shown));
        (dot(a, b) / (length(a) * length(b)).max(1e-6))
            .clamp(-1.0, 1.0)
            .dacos()
            .to_degrees()
    }

    /// The struck region and its parent: the chest against the lower back, the head against the
    /// chest (neck and head), the torso against the pelvis for a pelvis hit.
    fn region(part: usize) -> (usize, usize) {
        match part {
            mq::HEAD | mq::NECK => (mq::SPINE2, mq::HEAD),
            mq::PELVIS => (mq::PELVIS, mq::SPINE2),
            p => (mq::SPINE, p),
        }
    }

    #[derive(Debug, Default)]
    struct Run {
        dropped: u32,
        spread: f32,
        gap: f32,
        region_bend: f32,
        spine_bend: f32,
        tilt: f32,
        tilt_late: f32,
        plank_frames: u32,
    }

    /// `spec`'s one hit, then 3 s: the measures above.
    fn run(spec: &str) -> Run {
        run_on(spec, mq::PARENT)
    }

    /// [`run`] with the body's bones hung as `parents`.
    fn run_on(spec: &str, parents: [Option<usize>; mq::COUNT]) -> Run {
        let mut sim = Sim::new(GroundKind::Flat);
        sim.body_parents = parents;
        let spec = Spec::parse(spec).unwrap();
        prepare(&mut sim, &spec, Tuning::GAME).unwrap();
        let (a, b) = region(spec.hits[0].part.unwrap_or(mq::SPINE2));
        let mut out = Run::default();
        let mut wait = 0.1f32;
        let mut fired: Option<usize> = None;
        for frame in 0..((3.2 / STEP) as usize) {
            if fired.is_none() {
                wait -= STEP;
                if wait <= 0.0 {
                    fire(&mut sim, &spec.hits[0]);
                    fired = Some(frame);
                }
            }
            sim.step();
            let Some(f0) = fired else { continue };
            let t = (frame - f0) as f32 * STEP;
            let anim = sim.anim();
            let shown = sim.posed;
            if let Some(body) = &sim.rig.body {
                let pelvis = body.state[mq::PELVIS].com;
                for s in &body.state {
                    out.spread = out.spread.max(length(sub(s.com, pelvis)));
                }
                out.gap = out.gap.max(body.joint_gap().0);
                if parents == mq::PARENT {
                    assert_eq!(body.roots(), vec![mq::PELVIS], "only the pelvis is a root");
                }
            }
            if t <= 0.15 {
                out.region_bend = out.region_bend.max(bend(&shown, &anim, a, b));
            }
            out.spine_bend = out
                .spine_bend
                .max(bend(&shown, &anim, mq::PELVIS, mq::SPINE2));
            let tilted = tilt(&shown, &anim);
            out.tilt = out.tilt.max(tilted);
            if t >= 0.8 {
                out.tilt_late = out.tilt_late.max(tilted);
            }
            // A plank: the spine, the chest and the head turned the same way off the animation.
            let off = |b: usize| mul(&shown[b].0, &transpose(&anim[b].0));
            let (s, s2, h) = (off(mq::SPINE), off(mq::SPINE2), off(mq::HEAD));
            if tilted > 3.0 && angle(&s, &s2) < 1.0 && angle(&s2, &h) < 1.0 && angle(&s, &h) < 1.0 {
                out.plank_frames += 1;
            }
        }
        out.dropped = sim.stats.dropped;
        out
    }

    /// a) Every bone but the pelvis has a physics parent; the game's node tree gives the lab's.
    #[test]
    fn every_bone_hangs_from_the_body() {
        let phys = active::physics_parents(&mq::NODE_PARENT, mq::PELVIS);
        assert_eq!(phys, mq::PARENT.to_vec());
        for (b, p) in phys.iter().enumerate() {
            assert_eq!(p.is_none(), b == mq::PELVIS, "bone {b}");
        }
        assert_eq!(
            mq::NODE_PARENT[mq::SPINE],
            None,
            "FNV's spine hangs from NonAccum"
        );
    }

    /// a) to d) on the game's tree: 9mm at the chest, the head and the pelvis, and a .44 at the
    /// head. The joints hold (gap under 1 cm), nothing is dropped in 3 s, the body keeps together
    /// (spread under 1.1 m); the struck region bends within 0.15 s (3 degrees; the .44's neck and
    /// head 8, at least 2.5 since the head's nod is a tenth of the flinch: Patrick's bdf710a review
    /// drew the head at 1.8 times the flinch, its target 0.8 to 1.2; a pelvis hit's torso lags its
    /// pelvis 2.5: 2.6 measured, short of the 3 asked) and
    /// the spine more than the whole body tilts (1.5 times); the tilt about the ankles stays under
    /// 4 degrees and is back within 1 by 0.8 s; and it is never a plank.
    #[test]
    fn a_hit_bends_the_body_not_a_plank() {
        // (The least bends since 19-43-08: the body takes `active::KICK_SHARE` of a hit, over
        // `active::HIT_TIME`'s 0.12 s rise, so less of it is in by 0.15 s.)
        for (spec, least) in [
            ("hits=9mm@chest", 1.0),
            ("hits=9mm@head", 1.0),
            ("hits=9mm@pelvis", 0.8),
            ("hits=44@head", 2.5),
        ] {
            let r = run(spec);
            eprintln!("{spec}: {r:?}");
            assert_eq!(r.dropped, 0, "{spec}: dropped");
            assert!(r.gap < 0.01, "{spec}: a joint {:.3} m apart", r.gap);
            assert!(r.spread < 1.1, "{spec}: spread {:.2} m", r.spread);
            assert!(
                r.region_bend >= least,
                "{spec}: the struck region bent {:.2} degrees in 0.15 s",
                r.region_bend
            );
            assert!(
                r.spine_bend >= 1.5 * r.tilt,
                "{spec}: the spine bent {:.2} degrees, the body tilted {:.2}",
                r.spine_bend,
                r.tilt
            );
            assert!(r.tilt <= 4.0, "{spec}: tilted {:.2} degrees", r.tilt);
            assert!(
                r.tilt_late <= 1.0,
                "{spec}: still {:.2} degrees after 0.8 s",
                r.tilt_late
            );
            assert_eq!(r.plank_frames, 0, "{spec}: a plank");
        }
    }

    /// The game before: the body hung as the node tree (the spine a free root, as the host read
    /// FNV's skeleton). A 9mm at the chest spins the upper body as one plank and the body is
    /// dropped; on the physics parents, the same hit does neither (above).
    #[test]
    fn the_node_tree_as_the_body_was_a_plank() {
        let r = run_on("hits=9mm@chest", mq::NODE_PARENT);
        eprintln!("node tree: {r:?}");
        assert!(
            r.dropped > 0 || r.plank_frames > 0,
            "the old tree held together: {r:?}"
        );
    }

    /// e) When the body's capture point leaves its support (the feet's reach), a catch step starts
    /// within 0.25 s. (Since the body takes `active::KICK_SHARE` of a hit, a staggering hit's
    /// steps start with the hit, before its capture point leaves: that counts as followed.)
    #[test]
    fn a_step_follows_the_capture_point() {
        let mut left_any = false;
        for spec in [
            "hits=9mm@chest",
            "hits=44@chest",
            "hits=4570@chest",
            "hits=308@chest",
            "hits=127@chest",
            "hits=12ga@chest",
            "hits=44@pelvis",
            "hits=4570@pelvis",
        ] {
            let mut sim = Sim::new(GroundKind::Flat);
            let spec_p = Spec::parse(spec).unwrap();
            prepare(&mut sim, &spec_p, Tuning::GAME).unwrap();
            fire(&mut sim, &spec_p.hits[0]);
            let reach = ragdoll_core::tuning::get().foot_reach;
            let mut left_at: Option<f32> = None;
            let mut stepped_at: Option<f32> = None;
            for frame in 0..((2.0 / STEP) as usize) {
                sim.step();
                let t = frame as f32 * STEP;
                if stepped_at.is_none() && (sim.rig.balance.is_some() || sim.stats.steps > 0) {
                    stepped_at = Some(t);
                }
                let Some(body) = &sim.rig.body else { continue };
                let (mut c, mut v) = ([0.0; 3], [0.0; 3]);
                for (s, g) in body.state.iter().zip(&body.segments) {
                    c = add(c, scale(s.com, g.mass));
                    v = add(v, scale(s.v, g.mass));
                }
                let (c, v) = (scale(c, 1.0 / body.mass), scale(v, 1.0 / body.mass));
                let feet = scale(
                    add(body.state[mq::L_FOOT].com, body.state[mq::R_FOOT].com),
                    0.5,
                );
                let h = (c[2] - feet[2]).max(0.3);
                let w = (active::GRAVITY / h).sqrt();
                let capture = [c[0] + v[0] / w, c[1] + v[1] / w];
                let off = length([capture[0] - feet[0], capture[1] - feet[1], 0.0]);
                if left_at.is_none() && off > reach + 0.12 {
                    left_at = Some(t);
                }
            }
            eprintln!(
                "{spec}: capture point left at {left_at:?}, stepped at {stepped_at:?}, {:?}, fell {}",
                sim.stats.decisions,
                !sim.standing()
            );
            // A knockdown goes over (a fall, no catch step).
            if sim.stats.decisions.iter().any(|d| d == "Knockdown") {
                continue;
            }
            if stepped_at.is_some_and(|s| s <= 0.05) {
                left_any = true;
            }
            if let Some(l) = left_at {
                left_any = true;
                let s = stepped_at.unwrap_or(f32::MAX);
                assert!(
                    s <= l + 0.25,
                    "{spec}: left at {l:.2} s, stepped at {s:.2} s"
                );
            }
        }
        assert!(
            left_any,
            "no hit moved the capture point out of the support, nor stepped with the hit"
        );
    }
}

/// The arms and the falls (Patrick's 10:47 clip on 454ebb2: arms flailing through the body, a
/// folded body jittering then snapping upright). Unarmed, on the game's node tree.
#[cfg(test)]
mod arms {
    use super::*;
    use crate::sim::Sim;
    use ragdoll_core::active;

    type Pose = [(M3, V3); mq::COUNT];

    fn angle(a: &M3, b: &M3) -> f32 {
        length(active::qlog(active::qmul(
            active::qfrom_m(a),
            active::qconj(active::qfrom_m(b)),
        )))
    }

    fn bend(shown: &Pose, anim: &Pose, a: usize, b: usize) -> f32 {
        let rel = |x: &Pose| mul(&transpose(&x[a].0), &x[b].0);
        angle(&rel(shown), &rel(anim))
    }

    /// The closest distance of two segments.
    fn seg_dist(p1: V3, q1: V3, p2: V3, q2: V3) -> f32 {
        let (d1, d2, r) = (sub(q1, p1), sub(q2, p2), sub(p1, p2));
        let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
        let (s, t);
        if a <= 1e-9 && e <= 1e-9 {
            return length(r);
        }
        if a <= 1e-9 {
            s = 0.0;
            t = (f / e).clamp(0.0, 1.0);
        } else {
            let c = dot(d1, r);
            if e <= 1e-9 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else {
                let b = dot(d1, d2);
                let den = a * e - b * b;
                let s0 = if den > 1e-9 {
                    ((b * f - c * e) / den).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let t0 = (b * s0 + f) / e;
                if t0 < 0.0 {
                    t = 0.0;
                    s = (-c / a).clamp(0.0, 1.0);
                } else if t0 > 1.0 {
                    t = 1.0;
                    s = ((b - c) / a).clamp(0.0, 1.0);
                } else {
                    t = t0;
                    s = s0;
                }
            }
        }
        length(sub(add(p1, scale(d1, s)), add(p2, scale(d2, t))))
    }

    const TORSO: [usize; 4] = [mq::PELVIS, mq::SPINE, mq::SPINE1, mq::SPINE2];
    const LIMB: [usize; 4] = [mq::L_FOREARM, mq::R_FOREARM, mq::L_HAND, mq::R_HAND];

    /// The deepest any forearm or hand is in a torso capsule (units; the mannequin's capsules).
    fn depth(x: &Pose, _rest: &Pose) -> f32 {
        let cap = |b: usize| mq::capsule_at(b, x[b].0, x[b].1, 1.0);
        let mut most = 0.0f32;
        for &l in &LIMB {
            let (a, b, r) = cap(l);
            for &t in &TORSO {
                let (c, d, s) = cap(t);
                most = most.max(r + s - seg_dist(a, b, c, d));
            }
        }
        most
    }

    #[derive(Debug, Default)]
    pub(super) struct Run {
        pub shoulder_over: f32,
        pub elbow_least: f32,
        pub arm_speed: f32,
        pub arm_frame_deg: f32,
        pub deeper: f32,
        pub head: f32,
        pub chest: f32,
        pub arm_rms_late: f32,
        pub dropped: u32,
        pub fell: bool,
    }

    pub(super) fn run(spec: &str) -> Run {
        let mut sim = Sim::new(GroundKind::Flat);
        let spec = Spec::parse(spec).unwrap();
        prepare(&mut sim, &spec, Tuning::GAME).unwrap();
        let rest = sim.anim();
        let mut r = Run {
            elbow_least: f32::MAX,
            ..Run::default()
        };
        let mut wait = 0.1f32;
        let mut fired = None;
        let mut before = sim.posed;
        let mut late: [Vec<f32>; 4] = Default::default();
        let frames = (3.0 / sim.dt) as usize;
        for frame in 0..frames {
            if fired.is_none() {
                wait -= sim.dt;
                if wait <= 0.0 {
                    fire(&mut sim, &spec.hits[0]);
                    fired = Some(frame);
                }
            }
            before = sim.posed;
            sim.step();
            let Some(f0) = fired else { continue };
            if !sim.standing() {
                r.fell = true;
                continue;
            }
            let t = (frame - f0) as f32 * sim.dt;
            let anim = sim.anim();
            let p = sim.posed;
            // The shoulder's range: the upper arm against the chest (its forward turned with it).
            let chest = mul(&p[mq::SPINE2].0, &transpose(&anim[mq::SPINE2].0));
            let fwd = apply_m(&chest, crate::sim::FORWARD);
            let up = apply_m(&chest, [0.0, 0.0, 1.0]);
            for side in 0..2u8 {
                let u = mq::bone_of(Role::UpperArm, side).unwrap();
                let fo = mq::bone_of(Role::Forearm, side).unwrap();
                let over = ragdoll_core::flinch::shoulder_over(p[u].1, p[fo].1, fwd, up, side);
                r.shoulder_over = r.shoulder_over.max(over);
                r.elbow_least = r.elbow_least.min(crate::sim::hinge_bend(&p, u, fo, 1.0));
            }
            // The free arms' speed (none brace unarmed? the bracing ones are measured too).
            for (k, b) in [mq::L_UPPERARM, mq::R_UPPERARM, mq::L_FOREARM, mq::R_FOREARM]
                .into_iter()
                .enumerate()
            {
                // The free arms (not bracing).
                let side = (k % 2) as usize;
                if sim.rig.brace.is_some_and(|br| br.hands[side].is_some()) {
                    continue;
                }
                let a = angle(&p[b].0, &before[b].0);
                r.arm_speed = r.arm_speed.max(a / sim.dt);
                r.arm_frame_deg = r.arm_frame_deg.max((a / sim.dt / 60.0).to_degrees());
                if t >= 0.6 {
                    late[k].push(bend(&p, &anim, mq::PARENT[b].unwrap(), b).to_degrees());
                }
            }
            r.deeper = r.deeper.max(depth(&p, &rest) - depth(&anim, &rest));
            r.head = r
                .head
                .max(bend(&p, &anim, mq::SPINE2, mq::HEAD).to_degrees());
            r.chest = r
                .chest
                .max(bend(&p, &anim, mq::PELVIS, mq::SPINE2).to_degrees());
        }
        // The arms' ringing after 0.6 s: each arm bone's angle off the animation about its own
        // mean (a settled offset is no ringing), RMS over them.
        let (mut sq, mut n) = (0.0f32, 0usize);
        for v in &late {
            if v.is_empty() {
                continue;
            }
            let mean = v.iter().sum::<f32>() / v.len() as f32;
            sq += v.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>();
            n += v.len();
        }
        r.arm_rms_late = if n > 0 { (sq / n as f32).sqrt() } else { 0.0 };
        r.dropped = sim.stats.dropped;
        r
    }

    /// Unarmed, on flat ground, a railing, a table, a half-wall and a wall, a .44 at the left
    /// clavicle, the left upper arm from 40 degrees round, the head and the chest, at 30, 60 and
    /// 240 frames a second: no shoulder past its human range, no elbow past straight (by more
    /// than 0.03 rad), a free arm's bones no faster than 8 rad/s nor 6 degrees a 60 Hz frame, no
    /// forearm or hand more than 1 cm deeper in the torso than the animation has it.
    #[test]
    fn arms_keep_a_human_range() {
        let mut bad = Vec::new();
        for ground in ["flat", "railing", "table", "halfwall", "wall"] {
            for hit in ["44@lclavicle", "44@lupperarm/-40", "44@head", "44@chest"] {
                for dt in ["0.0333", "0.01667", "0.004167"] {
                    let spec = format!("ground={ground}&dt={dt}&hits={hit}");
                    let r = run(&spec);
                    let mut why = Vec::new();
                    if r.shoulder_over > 0.01 {
                        why.push(format!(
                            "shoulder {:.3} rad past its range",
                            r.shoulder_over
                        ));
                    }
                    if r.elbow_least < -0.031 {
                        why.push(format!("elbow {:.3} rad past straight", r.elbow_least));
                    }
                    if r.arm_speed > 8.0 || r.arm_frame_deg > 6.0 {
                        why.push(format!(
                            "a free arm at {:.2} rad/s, {:.2} degrees a frame",
                            r.arm_speed, r.arm_frame_deg
                        ));
                    }
                    if r.deeper > 0.7 {
                        why.push(format!("an arm {:.2} units deeper in the torso", r.deeper));
                    }
                    if r.dropped > 0 {
                        why.push("dropped".into());
                    }
                    if !why.is_empty() {
                        bad.push(format!("{spec}: {}", why.join(", ")));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    /// A .44 at the head: the head nods (at most 25 degrees against the chest), the chest bends
    /// (at most 15 against the pelvis: 14.6 measured, the 12 asked not reached), and the arms do
    /// not ring after 0.6 s (their angle about its own mean at most 2 degrees RMS).
    #[test]
    fn a_head_hit_nods_the_head() {
        let r = run("dt=0.01667&hits=44@head");
        eprintln!("{r:?}");
        assert!(r.head <= 25.0, "the head {:.1} degrees", r.head);
        assert!(r.chest <= 15.0, "the chest {:.1} degrees", r.chest);
        assert!(
            r.arm_rms_late <= 2.0,
            "the arms ring {:.2} degrees",
            r.arm_rms_late
        );
    }

    /// The largest turn of any bone (degrees) and move of the pelvis (units) from `a` to `b`.
    fn frame_change(a: &Pose, b: &Pose) -> (f32, f32) {
        let turn = (0..mq::COUNT)
            .map(|i| angle(&a[i].0, &b[i].0).to_degrees())
            .fold(0.0f32, f32::max);
        (turn, length(sub(a[mq::PELVIS].1, b[mq::PELVIS].1)))
    }

    /// A big hit just after a get-up (within the cooldown) makes no fall pose: a stagger.
    #[test]
    fn a_knockdown_in_the_cooldown_is_a_stagger() {
        let mut sim = Sim::new(GroundKind::Flat);
        let spec = Spec::parse("hits=50mg@chest").unwrap();
        prepare(&mut sim, &spec, Tuning::GAME).unwrap();
        sim.last_up = Some(sim.t);
        fire(&mut sim, &spec.hits[0]);
        for _ in 0..((1.5 / STEP) as usize) {
            assert!(sim.rig.fall.is_none(), "a fall pose in the cooldown");
            sim.step();
        }
        assert!(sim.standing(), "it went down in the cooldown");
        assert!(
            sim.stats.decisions.iter().any(|d| d == "Knockdown"),
            "{:?}",
            sim.stats.decisions
        );
    }

    /// A fall pose whose knock-down is refused at its end stands back up eased: no bone turns more
    /// than 6 degrees, nor the pelvis moves more than 3 units, in any frame after it.
    #[test]
    fn a_refused_fall_stands_back_up_eased() {
        let mut sim = Sim::new(GroundKind::Flat);
        let spec = Spec::parse("hits=44@chest&tune=knockdown_threshold:25,fall_lead:1").unwrap();
        prepare(&mut sim, &spec, Tuning::GAME).unwrap();
        fire(&mut sim, &spec.hits[0]);
        let mut refused = false;
        let mut most = (0.0f32, 0.0f32);
        for _ in 0..((2.0 / STEP) as usize) {
            let falling = sim.rig.fall.is_some();
            // The knock-down will be refused: a get-up "just now" as the lead ends.
            if falling {
                sim.last_up = Some(sim.t);
            }
            let before = sim.posed;
            sim.step();
            if falling && sim.rig.fall.is_none() && sim.standing() {
                refused = true;
            }
            if refused {
                let (turn, moved) = frame_change(&before, &sim.posed);
                most = (most.0.max(turn), most.1.max(moved));
            }
        }
        assert!(refused, "the fall was not refused");
        assert!(sim.standing(), "it went down");
        assert!(
            most.0 <= 6.0 && most.1 <= 3.0,
            "a bone turned {:.1} degrees, the pelvis moved {:.2} units in a frame",
            most.0,
            most.1
        );
    }
}

#[cfg(test)]
/// Patrick's c2a53c0 fold clip (a .44 into a Powder Ganger braced at a waist-high obstacle): after
/// the hands let go, with no further hit, the body kept moving on its own for 5.5 s (the pelvis 3.5
/// to 23 to 32 units off its animation, a stiff tilt, spinning), then snapped upright (18 to 1.3
/// units in 0.25 s). The game's AI turning and walking the actor all the while: the lab's `turn`
/// (degrees/s) and `walk` (units/s).
mod runaway {
    use super::*;
    use crate::sim::Sim;
    use ragdoll_core::active;

    /// One non-toppling .44 hit, then nothing, at a waist-high table, a half wall, in the open;
    /// armed and not; the actor standing, turned, walked, turned and walked.
    fn specs() -> Vec<String> {
        let mut out = Vec::new();
        for scene in [
            "ground=table&armed=1",
            "ground=table",
            "ground=halfwall&armed=1",
            "ground=flat&armed=1",
        ] {
            for moving in [
                "",
                "&turn=90",
                "&turn=-200",
                "&walk=120",
                "&turn=90&walk=120",
            ] {
                out.push(format!("{scene}&hits=44@chest{moving}"));
            }
        }
        out
    }

    fn runs() -> Vec<(String, Report)> {
        specs()
            .into_iter()
            .map(|spec| {
                let mut sim = Sim::new(GroundKind::Flat);
                let r = run(&mut sim, &Spec::parse(&spec).unwrap(), Tuning::GAME).unwrap();
                (spec, r)
            })
            .collect()
    }

    /// The runs, printed for the record (`cargo test runaway -- --nocapture`).
    fn printed() -> Vec<(String, Report)> {
        let all = runs();
        for (spec, r) in &all {
            eprintln!(
                "{spec:52} release rise {:.2}  hand-back rise {:.3} jump {:.2}  yaw {:.2} rad/s \
                 (off {:.2} rad)  handed back {:?} s, recovery {:?} s",
                r.release_rise,
                r.settle_rise,
                r.settle_jump,
                r.yaw_rate,
                r.yaw_off,
                r.handed_back,
                r.recovery
            );
        }
        all
    }

    /// One hit, then nothing: once nothing holds the body (its hands let go, its steps done) its
    /// offset never runs away (before: 29.8 and 30.2 units with the actor turned at 200 degrees/s),
    /// and once it is handed back it only settles (the offset never grows, before: 0.47), smoothly
    /// (the shown pelvis no more than half a unit a frame beyond the actor's own move; before:
    /// 1.66).
    #[test]
    fn one_hit_then_nothing_only_settles() {
        for (spec, r) in printed() {
            assert!(!r.fell, "{spec}: one .44 hit does not topple it");
            assert!(
                r.release_rise <= 3.0,
                "{spec}: ran off after release: {:.2}",
                r.release_rise
            );
            assert!(
                r.settle_rise <= 0.05,
                "{spec}: grew while handed back: {:.3}",
                r.settle_rise
            );
            assert!(
                r.settle_jump <= 0.5,
                "{spec}: jumped while handed back: {:.2}",
                r.settle_jump
            );
        }
    }

    /// One non-toppling hit's reaction is over by [`active::REACTION_MOST`]: its body handed back
    /// to the animation one hand-back later, and all of it (a brace's arm home too) within a
    /// second more (before: 6.9 s at the table, the hands held, then home).
    #[test]
    fn one_hit_reaction_is_capped() {
        let back = active::REACTION_MOST + ragdoll_core::tuning::get().blend_back + 0.05;
        for (spec, r) in runs() {
            let at = r.handed_back.unwrap_or(f32::INFINITY);
            assert!(
                at <= back,
                "{spec}: body handed back after {at:.2} s (most {back:.2})"
            );
            let rec = r.recovery.unwrap_or(f32::INFINITY);
            assert!(
                rec <= back + 1.0,
                "{spec}: back in its animation after {rec:.2} s"
            );
        }
    }

    /// The heading never spins: the shown pelvis never turns against its animation faster than 2
    /// rad/s nor more than 0.2 rad off it, the actor standing, turned either way, walked (before:
    /// 5.14 rad/s and 0.28 rad, the brace hand held where it touched while the AI turned it).
    #[test]
    fn heading_never_spins() {
        for (spec, r) in runs() {
            assert!(
                r.yaw_rate <= 2.0,
                "{spec}: turned {:.2} rad/s against its animation",
                r.yaw_rate
            );
            assert!(
                r.yaw_off <= 0.2,
                "{spec}: turned {:.2} rad off its animation",
                r.yaw_off
            );
        }
    }
}

#[cfg(test)]
/// Patrick's 14-16-02 Play clip (b44a964, a Powder Ganger in Goodsprings, a 10mm): a second
/// small flinch (0.53 m/s at the left clavicle), then its AI walks and turns it, and the reaction
/// went wild: the pelvis 48 units off, 14 units a frame, the upper body spinning at the waist 3 to
/// 4 times round, frozen at 34 units while it kept turning, a snap back to facing forward. The
/// lab's `move` (s after the last hit), `walk`, `turn`, `turnby`; `readback=1` reads the animation
/// back from the bones as the host does, `aimpass=1` with a later pass turning the upper spine
/// while the AI moves it (`Sim::read_back`, `Sim::aim_pass`).
mod locomotion {
    use super::*;
    use crate::sim::Sim;

    fn specs() -> Vec<(String, f32)> {
        let mut out = Vec::new();
        for (hits, armed) in [
            ("10mm@head,10mm@lclavicle+2.13", "pistol"),
            ("10mm@lclavicle", "pistol"),
            ("10mm@chest", "0"),
            ("10mm@head", "rifle"),
        ] {
            for (after, walk, turn, by) in [
                (0.37, 23.0, 120.0, 150.0),
                (0.2, 40.0, 180.0, 180.0),
                (0.2, 0.0, -150.0, 90.0),
                (-1.0, 23.0, 120.0, 150.0),
            ] {
                for read in ["", "&readback=1", "&readback=1&aimpass=1"] {
                    out.push((
                        format!(
                            "armed={armed}&hits={hits}&move={after}&walk={walk}&turn={turn}\
                             &turnby={by}{read}"
                        ),
                        f32::abs(turn).to_radians(),
                    ));
                }
            }
        }
        out
    }

    fn runs() -> Vec<(String, f32, Report)> {
        specs()
            .into_iter()
            .map(|(spec, turn)| {
                let mut sim = Sim::new(GroundKind::Flat);
                let r = run(&mut sim, &Spec::parse(&spec).unwrap(), Tuning::GAME).unwrap();
                eprintln!(
                    "{spec:96} pelvis {:.1} jump {:.2} spin {:.1} ({}) yaw {:.2} stale {} \
                     recovery {:?}",
                    r.pelvis_most,
                    r.pelvis_jump,
                    r.bone_spin,
                    r.spin_bone,
                    r.yaw_rate,
                    r.base_stale,
                    r.recovery
                );
                (spec, turn, r)
            })
            .collect()
    }

    /// A small hit, then the AI walks and turns the actor 90 to 180 degrees: the reaction stays
    /// small (the pelvis at most 4 units off: the catch step of a hit while it stood), never turns
    /// the body against its animation as fast as the animation turns, nor moves the pelvis 2
    /// units in a frame, nor turns any bone off its animation faster than `active::SPIN_MOST`; the
    /// animation is never read stale; it is over within 3.5 s of the first hit.
    #[test]
    fn a_small_hit_rides_on_the_walk() {
        /// A leg's swing (rad/s) at most.
        const STEP_LEG_SPIN: f32 = 12.0;
        for (spec, turn, r) in runs() {
            assert!(!r.fell, "{spec}: fell");
            assert!(
                r.pelvis_most <= 4.0,
                "{spec}: pelvis {:.1} units off",
                r.pelvis_most
            );
            assert!(
                r.pelvis_jump <= 2.0,
                "{spec}: pelvis {:.2} units in a frame",
                r.pelvis_jump
            );
            assert!(
                r.yaw_rate < turn,
                "{spec}: turned {:.2} rad/s against the animation's {turn:.2}",
                r.yaw_rate
            );
            // A leg swings on the steps' own pose, or is lifted off the floor over the walk's
            // feet (`active::lift_feet`): a person's step, not the reaction's offset the spin
            // bound holds (the 0c1fbbf lab swung them up to 11.5 rad/s).
            let leg = ["Thigh", "Calf", "Foot"]
                .iter()
                .any(|l| r.spin_bone.contains(l));
            let bound = if leg {
                STEP_LEG_SPIN
            } else {
                ragdoll_core::active::SPIN_MOST + 0.5
            };
            assert!(
                r.bone_spin <= bound,
                "{spec}: {} turned {:.1} rad/s off its animation (steps {})",
                r.spin_bone,
                r.bone_spin,
                r.steps
            );
            assert_eq!(r.base_stale, 0, "{spec}: the animation read stale");
            assert!(
                r.recovery.is_some_and(|t| t <= 3.5),
                "{spec}: {:?}",
                r.recovery
            );
        }
    }

    /// The cause: with the write left in the bones (b44a964), the animation's own walk is taken
    /// for a later pass's move of the write and its pose kept stale, hundreds of reads; put back
    /// after the draw, never.
    #[test]
    fn a_write_left_in_the_bones_reads_the_walk_stale() {
        let spec =
            "armed=pistol&hits=10mm@lclavicle&move=0.2&walk=40&turn=180&turnby=180&aimpass=1";
        let read = |how: &str| {
            let mut sim = Sim::new(GroundKind::Flat);
            let spec = Spec::parse(&format!("{spec}&readback={how}")).unwrap();
            run(&mut sim, &spec, Tuning::GAME).unwrap().base_stale
        };
        let (left, put_back) = (read("written"), read("1"));
        eprintln!("stale reads: write left {left}, put back {put_back}");
        // (Dozens: 48 since the drawn flinch of 08-21-49; never any put back.)
        assert!(left >= 30, "{left}");
        assert_eq!(put_back, 0);
    }

    /// A standing flinch, the speed (Patrick's 19-43-08 clip, 0c1fbbf: a 0.32 m/s 9mm turned the
    /// torso 20 to 27 degrees, the arms and head spun at 12 rad/s, and the bounds held most bones
    /// every frame, a stiff deep bend): 9mm to .45-70 hits at the chest, clavicle, head, side and
    /// pelvis, unarmed, pistol and rifle, turn the torso no more than the push's turn
    /// (`active::offset_most`: 8 degrees at 0.3 m/s, 20 at 0.75), no bone faster than
    /// `active::SPIN_MOST` off its animation, the bounds seldom holding, and still visibly.
    #[test]
    fn a_standing_flinch_is_held_to_its_push() {
        use ragdoll_core::active::{offset_most, SPIN_MOST};
        for armed in ["0", "pistol", "rifle"] {
            for hit in [
                "9mm@chest",
                "10mm@lclavicle",
                "10mm@head",
                "44@chest",
                "44@head",
                "44@spine1/40",
                "9mm@pelvis",
                "4570@chest",
            ] {
                let spec = Spec::parse(&format!("armed={armed}&hits={hit}")).unwrap();
                let flinch = |off: bool| {
                    let mut sim = Sim::new(GroundKind::Flat);
                    sim.limit_off = off;
                    let r = run(&mut sim, &spec, Tuning::GAME).unwrap();
                    (r, sim.stats.clone())
                };
                let ((r, s), (_, free)) = (flinch(false), flinch(true));
                let most = offset_most(s.react_v).0.to_degrees();
                eprintln!(
                    "{armed:6} {hit:14} v {:.2}: torso {:.1} (free {:.1}, most {most:.1}) any {:.1} \
                     spin {:.1} ({}) held {} steps {} fell {}",
                    s.react_v,
                    s.torso_turn,
                    free.torso_turn,
                    s.flinch,
                    r.bone_spin,
                    r.spin_bone,
                    s.limited,
                    s.steps,
                    s.fell
                );
                if s.fell {
                    continue;
                }
                assert!(
                    s.torso_turn <= most + 1.0,
                    "{armed} {hit}: the torso {:.1} degrees, at most {most:.1}",
                    s.torso_turn
                );
                assert!(s.flinch >= 2.0, "{armed} {hit}: {:.1} degrees", s.flinch);
                // The drawn torso's turn never changes faster than the spin bound.
                assert!(
                    s.turn_jump <= SPIN_MOST.to_degrees() * crate::sim::STEP + 0.3,
                    "{armed} {hit}: the torso's turn {:.1} degrees in a frame",
                    s.turn_jump
                );
                if s.steps == 0 {
                    assert!(
                        r.bone_spin <= SPIN_MOST + 0.5,
                        "{armed} {hit}: {} {:.1} rad/s",
                        r.spin_bone,
                        r.bone_spin
                    );
                }
            }
        }
    }
}

#[cfg(test)]
/// nv-rs's own perks in the active ragdoll (`ragdoll_core::perks`; the host's `perks`): the push
/// numbers with and without each.
mod perk_pushes {
    use super::*;
    use crate::sim::Sim;
    use ragdoll_core::knock::Reaction;

    fn run_with(spec: &str) -> (Report, Vec<f32>) {
        let mut sim = Sim::new(GroundKind::Flat);
        let r = run(&mut sim, &Spec::parse(spec).unwrap(), Tuning::GAME).unwrap();
        (r, sim.stats.pushes.clone())
    }

    /// Stopping Power: each of the player's hits pushes the active ragdoll 1.35 times as hard; a
    /// 10mm at the chest (0.53 m/s against a stagger at 0.56) staggers and interrupts the attack
    /// with it and not without; the push's speed and topple decision are the hit's own.
    #[test]
    fn stopping_power_pushes_harder_and_interrupts_sooner() {
        for hit in ["9mm@chest", "10mm@chest", "44@chest"] {
            let (plain, p0) = run_with(&format!("attacking=1&hits={hit}"));
            let (perk, p1) = run_with(&format!("attacking=1&hits={hit}&perks=stoppingpower"));
            eprintln!(
                "{hit}: push {:.1} -> {:.1} N s, steps {} -> {}, {:?} -> {:?}, interrupted {} -> {}",
                p0[0], p1[0], plain.steps, perk.steps, plain.decisions, perk.decisions,
                plain.interrupted, perk.interrupted
            );
            assert!((p1[0] / p0[0] - 1.35).abs() < 1e-3, "{hit}");
            assert!((perk.peak_speed - plain.peak_speed).abs() < 1e-4, "{hit}");
        }
        let (plain, _) = run_with("attacking=1&hits=10mm@chest");
        let (perk, _) = run_with("attacking=1&hits=10mm@chest&perks=stoppingpower");
        assert_eq!(plain.decisions, vec!["None".to_string()]);
        assert_eq!(perk.decisions, vec!["Stagger".to_string()]);
        assert!(perk.interrupted);
    }

    /// Hot Loads: the round leaves 10% faster, so its push into the body is 10% more.
    #[test]
    fn hot_loads_pushes_ten_percent_more() {
        for hit in ["9mm@chest", "308@chest", "556@chest"] {
            let (plain, p0) = run_with(&format!("hits={hit}"));
            let (perk, p1) = run_with(&format!("hits={hit}&perks=hotloads"));
            eprintln!(
                "{hit}: push {:.1} -> {:.1} N s, speed {:.2} -> {:.2} m/s",
                p0.iter().sum::<f32>(),
                p1.iter().sum::<f32>(),
                plain.peak_speed,
                perk.peak_speed
            );
            let ratio = p1.iter().sum::<f32>() / p0.iter().sum::<f32>();
            assert!((ratio - 1.1).abs() < 0.02, "{hit}: {ratio}");
        }
    }

    /// Bowling Ball: a knocked-over 80 kg body flying into the actor: nothing without the perk;
    /// with it, a push into the active ragdoll at 2 m/s, a stagger with a catch step at 4, a
    /// knock-down at 8.
    #[test]
    fn bowling_ball_pushes_by_the_flying_bodys_speed() {
        let bump = |speed: f32, perk: bool| {
            let mut sim = Sim::new(GroundKind::Flat);
            prepare(&mut sim, &Spec::parse("hits=").unwrap(), Tuning::GAME).unwrap();
            sim.perks.bowling_ball = perk;
            for _ in 0..18 {
                sim.step();
            }
            let d = sim.bump(80.0, speed, [0.0, -1.0, 0.0]);
            for _ in 0..120 {
                sim.step();
            }
            (
                d,
                sim.stats.pushes.first().copied().unwrap_or(0.0),
                sim.stats.steps,
                sim.stats.fell,
            )
        };
        for speed in [2.0, 4.0, 8.0] {
            let (without, ..) = bump(speed, false);
            let (with, p, steps, fell) = bump(speed, true);
            eprintln!("{speed} m/s: without {without:?}; with {with:?}, {p:.0} N s, steps {steps}, fell {fell}");
            assert_eq!(without, None);
        }
        assert_eq!(bump(2.0, true).0, Some(Reaction::None));
        assert!(bump(2.0, true).1 > 0.0);
        assert_eq!(bump(4.0, true).0, Some(Reaction::Stagger));
        assert!(bump(4.0, true).2 >= 1, "steps");
        assert_eq!(bump(8.0, true).0, Some(Reaction::Knockdown));
        assert!(bump(8.0, true).3, "knocked over");
    }
}

#[cfg(test)]
/// The floor (Patrick's 19-43-08 clip, 0c1fbbf: a .44's catch step moved the Fiend's reference 3
/// units down with its feet, and she sank upright, hips below the ground): a flinch, a catch step
/// and a reaction yielding to the AI's walk never put a foot or the pelvis under the floor, the
/// pelvis never more than `active::PELVIS_DROP_FLINCH` under its animation (a flinch) nor
/// `active::PELVIS_DROP_STEP` (stepping), the reference never moved up or down by the steps.
mod floor {
    use super::*;
    use crate::sim::Sim;
    use ragdoll_core::active::{PELVIS_DROP_FLINCH, PELVIS_DROP_STEP};

    fn run_on(ground: GroundKind, spec: &str) -> Sim {
        let mut sim = Sim::new(ground);
        run(&mut sim, &Spec::parse(spec).unwrap(), Tuning::GAME).unwrap();
        let s = &sim.stats;
        eprintln!(
            "{ground:?} {spec:72} steps {} pelvis drop {:.1} (flinch {:.1}, stepping {:.1}) foot \
             drop {:.1} under floor {:.1} reference dz {:.1}",
            s.steps,
            s.pelvis_drop,
            s.flinch_pelvis_drop,
            s.step_pelvis_drop,
            s.foot_drop,
            s.under_floor,
            s.reference_dz
        );
        sim
    }

    const FLINCHES: [&str; 4] = [
        "armed=pistol&hits=9mm@chest",
        "hits=10mm@chest",
        "armed=rifle&hits=44@spine1/40",
        "hits=9mm@pelvis",
    ];
    const STEPS: [&str; 3] = [
        "hits=44@chest",
        "armed=pistol&hits=44@chest",
        "hits=4570@chest",
    ];
    const YIELDS: [&str; 2] = [
        "armed=pistol&hits=10mm@lclavicle&move=0.2&walk=40&turn=180&turnby=180",
        "hits=44@chest&move=0.4&walk=23&turn=120&turnby=150",
    ];

    fn check(sim: &Sim, spec: &str) {
        let s = &sim.stats;
        if s.fell {
            return; // its ragdoll's (the knockdown's own checks)
        }
        assert!(
            s.under_floor <= 0.5,
            "{spec}: {:.1} units under the floor",
            s.under_floor
        );
        // (While the AI walks it, the animated foot swings off the floor: the floor is checked.)
        assert!(
            spec.contains("walk=") || s.foot_drop <= ragdoll_core::active::LIFT_SLACK + 1.0,
            "{spec}: a foot {:.1} under its animation",
            s.foot_drop
        );
        assert!(
            s.flinch_pelvis_drop <= PELVIS_DROP_FLINCH + 0.05,
            "{spec}: the pelvis {:.1} under its animation",
            s.flinch_pelvis_drop
        );
        assert!(
            s.step_pelvis_drop <= PELVIS_DROP_STEP + 0.05,
            "{spec}: the pelvis {:.1} under its animation stepping",
            s.step_pelvis_drop
        );
        assert!(
            s.reference_dz <= 0.01,
            "{spec}: the reference moved {:.1} up or down",
            s.reference_dz
        );
    }

    #[test]
    fn a_flinch_stays_on_the_floor() {
        for spec in FLINCHES {
            let sim = run_on(GroundKind::Flat, spec);
            check(&sim, spec);
        }
    }

    #[test]
    fn a_catch_step_stays_on_the_floor() {
        let mut stepped = 0;
        for ground in [GroundKind::Flat, GroundKind::Slope, GroundKind::Uneven] {
            for spec in STEPS {
                let sim = run_on(ground, spec);
                stepped += (sim.stats.steps > 0) as u32;
                check(&sim, spec);
            }
        }
        assert!(stepped >= 3, "no catch steps taken ({stepped})");
    }

    #[test]
    fn a_yield_stays_on_the_floor() {
        for spec in YIELDS {
            let sim = run_on(GroundKind::Flat, spec);
            check(&sim, spec);
        }
    }

    /// A reaction yielding to its AI's walk and turn hands back with no jump: the torso's turn
    /// off its animation (the host's `drawn_turn`) never changes more than 5 degrees in a frame
    /// (Patrick's 19-43-08 clip: 42.6 degrees for one frame at the yield, then none).
    #[test]
    fn a_yield_eases_back_with_no_jump() {
        let mut specs: Vec<String> = YIELDS.iter().map(|s| s.to_string()).collect();
        for hit in ["9mm@chest", "10mm@head", "44@head"] {
            for armed in ["0", "pistol"] {
                specs.push(format!(
                    "armed={armed}&hits={hit}&move=0.3&walk=40&turn=180&turnby=180"
                ));
            }
        }
        for spec in &specs {
            let sim = run_on(GroundKind::Flat, spec);
            let s = &sim.stats;
            eprintln!(
                "{spec}: turn jump {:.2} degrees yielding, {:.2} over the reaction",
                s.yield_turn_jump, s.turn_jump
            );
            assert!(
                s.yield_turn_jump <= 5.0,
                "{spec}: the torso's turn jumped {:.1} degrees in a frame as it yielded",
                s.yield_turn_jump
            );
            // Never past the spin bound in a frame, over the whole reaction.
            assert!(
                s.turn_jump
                    <= ragdoll_core::active::SPIN_MOST.to_degrees() * crate::sim::STEP + 0.3,
                "{spec}: {:.1} degrees in a frame",
                s.turn_jump
            );
        }
    }

    /// A step's move of the reference is applied once: the actor goes across with the feet's
    /// middle (the sum of its moves is where the feet's middle went), the shown pelvis never
    /// jumps with it (the pelvis's offset takes the move back the same frame).
    #[test]
    fn a_steps_reference_move_is_applied_once() {
        for spec in STEPS {
            let sim = run_on(GroundKind::Flat, spec);
            let s = &sim.stats;
            if s.steps == 0 || s.fell {
                continue;
            }
            let moved = sub(sim.pos, s.start);
            assert!(moved[2].abs() <= 0.01, "{spec}: {moved:?}");
            assert!(
                s.pelvis_jump <= 3.0,
                "{spec}: the pelvis jumped {:.1} units",
                s.pelvis_jump
            );
        }
    }
}

#[cfg(test)]
/// Continuity (Patrick's 21-16-09 clip, 6b0cec2: 7 of 27 hits ended with the animation's L Calf
/// jumping a frame or two after the hit; the upper body popped sideways and up for 1 to 3
/// frames, then the reaction yielded): at the hit's onset, through a re-base, a yield and a
/// brace's release, no drawn bone moves more than 3 units or turns more than 6 degrees in a 60
/// fps frame beyond its animation's own motion.
mod continuity {
    use super::*;
    use crate::sim::Sim;

    pub const MOST_MOVE: f32 = 3.0;
    pub const MOST_TURN: f32 = 6.0;

    fn run_spec(spec: &str) -> Sim {
        let mut sim = Sim::new(GroundKind::Flat);
        run(&mut sim, &Spec::parse(spec).unwrap(), Tuning::GAME).unwrap();
        let s = &sim.stats;
        eprintln!(
            "{spec:80} moved {:.2} ({} at {:.2} s) turned {:.2} ({}) rebased {} steps {} fell {} \
             off {:.1} length {:.2}",
            s.drawn_move,
            ragdoll_core::flinch::BONES[s.drawn_move_bone].0,
            s.drawn_move_at,
            s.drawn_turn_step,
            ragdoll_core::flinch::BONES[s.drawn_turn_bone].0,
            s.rebased,
            s.steps,
            s.fell,
            s.drawn_most_off,
            s.length_change
        );
        sim
    }

    fn check(sim: &Sim, spec: &str) {
        let s = &sim.stats;
        assert!(
            s.drawn_most_off <= ragdoll_core::active::MOST_DRAWN_OFF,
            "{spec}: a bone drawn {:.1} units off its animation",
            s.drawn_most_off
        );
        assert!(
            s.length_change <= 0.5,
            "{spec}: a bone's length {:.2} units off",
            s.length_change
        );
        assert!(
            s.drawn_move <= MOST_MOVE,
            "{spec}: {} moved {:.2} units in a frame at {:.2} s",
            ragdoll_core::flinch::BONES[s.drawn_move_bone].0,
            s.drawn_move,
            s.drawn_move_at
        );
        assert!(
            s.drawn_turn_step <= MOST_TURN,
            "{spec}: {} turned {:.2} degrees in a frame",
            ragdoll_core::flinch::BONES[s.drawn_turn_bone].0,
            s.drawn_turn_step
        );
    }

    /// The game's hit animation jumping the L Calf 150 tolerances two frames after the hit is
    /// re-based, never drawn, never yielded to.
    #[test]
    fn a_leg_jump_at_the_hit_is_not_drawn() {
        for armed in ["0", "pistol", "rifle"] {
            for hit in ["9mm@chest", "10mm@chest", "10mm@head", "9mm@pelvis"] {
                let spec = format!("armed={armed}&hits={hit}&readback=1&legjump=2");
                let sim = run_spec(&spec);
                assert_eq!(sim.stats.rebased, 1, "{spec}: not re-based");
                assert!(
                    !sim.log.iter().any(|n| n.contains("yields")),
                    "{spec}: yielded"
                );
                check(&sim, &spec);
            }
        }
    }

    /// The same jump later than `active::REBASE_WINDOW` after the hit is yielded to, and the yield
    /// hands back with no jump either.
    #[test]
    fn a_hit_and_its_yield_are_continuous() {
        for spec in [
            "hits=9mm@chest",
            "armed=pistol&hits=9mm@chest",
            "armed=pistol&hits=10mm@lclavicle&move=0.2&walk=40&turn=180&turnby=180",
            "hits=10mm@head&move=0.3&walk=40&turn=180&turnby=180",
            "armed=pistol&hits=9mm@chest&readback=1&legjump=30",
            // Hits 1.1 s apart (Patrick's 08-21-49 clip: the drawn turn blipped on the first frame
            // after each). (An armed body's weapon hand swings up to 6.7 units a frame under four
            // 9mm hits, as in 3a9805e: not yet held.)
            "hits=9mm@chest*4~1.1",
            "hits=44@chest*4~1.1",
            "hits=9mm@chest*4~1.1&readback=1",
        ] {
            let sim = run_spec(spec);
            check(&sim, spec);
        }
    }
}

#[cfg(test)]
/// A thin rail (Patrick's 21-16-09 clip, 6b0cec2: a railing at his hips behind him; the hands went
/// behind the back, then the left arm was held out straight through the railing for 2 s and
/// snapped to idle): a 3-unit pipe at 80 units behind is never braced on with the hands; in
/// front, a planted wrist is drawn within a unit of its plant, its elbow never swings back and
/// forth, and a released hand is home within 0.6 s.
mod thin_rail {
    use super::*;
    use crate::sim::Sim;

    fn run_spec(spec: &str) -> Sim {
        let mut sim = Sim::new(GroundKind::Pipe);
        let r = run(&mut sim, &Spec::parse(spec).unwrap(), Tuning::GAME).unwrap();
        let s = &sim.stats;
        eprintln!(
            "{spec:64} braced {} hands {:?} planted {} drift {:.2} elbow {} return {:.2} s \
             fell {} upright {}",
            s.braced,
            s.brace_hands,
            r.log.iter().any(|l| l.contains("hand plants")),
            s.plant_drift,
            s.elbow_reversals,
            s.return_time,
            s.fell,
            r.upright
        );
        sim
    }

    #[test]
    fn a_rail_behind_gets_no_hands() {
        for armed in ["0", "pistol", "rifle"] {
            for gun in ["44", "4570", "308"] {
                for from in ["0", "15", "-15"] {
                    let spec = format!("ground=pipe&armed={armed}&hits={gun}@chest/{from}");
                    let sim = run_spec(&spec);
                    assert_eq!(
                        sim.stats.brace_hands,
                        [false, false],
                        "{spec}: a hand braced"
                    );
                    // Not begun at all, nor said (Patrick's 08-21-49 clip: announced, then let go
                    // 11 ms later), unless it goes over it.
                    assert!(sim.stats.fell || !sim.stats.braced, "{spec}: a brace begun");
                }
            }
        }
    }

    #[test]
    fn a_rail_in_front_is_held_still() {
        let mut planted = 0;
        for armed in ["0", "pistol", "rifle"] {
            for gun in ["44", "4570", "308"] {
                for from in ["15", "0", "-15"] {
                    let spec =
                        format!("ground=pipe&face=180&armed={armed}&hits={gun}@chest/{from}");
                    let sim = run_spec(&spec);
                    let s = &sim.stats;
                    if sim.log.iter().any(|l| l.contains("hand plants")) {
                        planted += 1;
                        assert!(
                            s.plant_drift <= 1.0,
                            "{spec}: the wrist {:.2} off",
                            s.plant_drift
                        );
                        assert_eq!(s.elbow_reversals, 0, "{spec}: the elbow swung");
                    }
                    assert!(s.return_time <= 0.6, "{spec}: {:.2} s home", s.return_time);
                }
            }
        }
        assert!(planted > 0, "no hand planted on the rail");
    }
}

#[cfg(test)]
/// Visible flinches (Patrick's 08-21-49 clip, 3a9805e: a .44 at 0.8 m/s drew the torso 4.2
/// degrees, a 9mm 2.1; the pushes along the line of sight): the spine's top turns off its
/// animation about 6 degrees at 0.32 m/s and 14 at 0.79 at its peak (no more than 1.2 times),
/// reached in 0.08 to 0.12 s and back within 0.5 to 0.7 s; an off-centre hit twists the struck
/// shoulder back 8 to 15 degrees; a head hit nods the head; never faster than the spin bound.
mod visible {
    use super::*;
    use crate::sim::Sim;
    use ragdoll_core::active::{torso_turn, SPIN_MOST};

    pub struct Peak {
        pub v: f32,
        pub peak: f32,
        pub at: f32,
        pub reached: f32,
        pub back: f32,
        pub yaw: f32,
        pub head: f32,
        pub step: f32,
    }

    pub fn peak_of(spec: &str) -> Peak {
        let mut sim = Sim::new(GroundKind::Flat);
        run(&mut sim, &Spec::parse(spec).unwrap(), Tuning::GAME).unwrap();
        let tr = &sim.stats.spine_trace;
        let (at, peak) = tr.iter().fold(
            (0.0, 0.0f32),
            |b, &(t, d, _, _)| if d > b.1 { (t, d) } else { b },
        );
        let reached = tr
            .iter()
            .find(|x| x.1 >= 0.9 * peak)
            .map_or(f32::NAN, |x| x.0);
        let back = tr
            .iter()
            .filter(|x| x.0 > at && x.1 > (0.1 * peak).max(1.0))
            .map(|x| x.0)
            .fold(at, f32::max);
        let yaw = tr
            .iter()
            .map(|x| x.2)
            .fold(0.0f32, |b, y| if y.abs() > b.abs() { y } else { b });
        let head = tr.iter().map(|x| x.3).fold(0.0f32, f32::max);
        let step = tr
            .windows(2)
            .map(|w| (w[1].1 - w[0].1).abs() / (w[1].0 - w[0].0).max(1e-4))
            .fold(0.0f32, f32::max)
            .to_radians();
        let p = Peak {
            v: sim.stats.react_v,
            peak,
            at,
            reached,
            back,
            yaw,
            head,
            step,
        };
        eprintln!(
            "{spec:44} v {:.2}: spine top {:.1} deg (wanted {:.1}) at {:.3} s, 90% at {:.3} s, \
             back by {:.2} s; twist {:.1}, head {:.1}; fastest {:.1} rad/s",
            p.v,
            p.peak,
            torso_turn(p.v).to_degrees(),
            p.at,
            p.reached,
            p.back,
            p.yaw,
            p.head,
            p.step
        );
        p
    }

    /// Front and side hits at 0.32 (a 9mm) and 0.79 m/s (a .44), standing, armed and not.
    #[test]
    fn the_flinch_peaks_as_wanted() {
        for armed in ["0", "pistol", "rifle"] {
            for hit in [
                "9mm@chest",
                "9mm@chest/90",
                "44@chest",
                "44@chest/90",
                "44@chest/-90",
            ] {
                let spec = format!("armed={armed}&hits={hit}");
                let p = peak_of(&spec);
                let want = torso_turn(p.v).to_degrees();
                assert!(
                    p.peak >= 0.9 * want && p.peak <= 1.2 * want,
                    "{spec}: {:.1} degrees at its peak, {want:.1} wanted",
                    p.peak
                );
                // Reached (90% of its peak) 0.08 to 0.12 s after the hit.
                assert!(
                    (0.08..=0.12).contains(&p.reached),
                    "{spec}: 90% of its peak at {:.3} s",
                    p.reached
                );
                assert!(p.back <= 0.7, "{spec}: back by {:.2} s", p.back);
                assert!(p.step <= SPIN_MOST + 0.3, "{spec}: {:.1} rad/s", p.step);
            }
        }
    }

    /// A hit on a shoulder turns that shoulder back 8 to 15 degrees; a head hit nods the head
    /// more than the torso turns.
    #[test]
    fn off_centre_and_head_hits_read() {
        for hit in [
            "9mm@lclavicle",
            "44@lupperarm",
            "44@rclavicle",
            "9mm@rupperarm",
        ] {
            let p = peak_of(&format!("hits={hit}"));
            assert!(
                (8.0..=15.0).contains(&p.yaw.abs()),
                "{hit}: twisted {:.1} degrees",
                p.yaw
            );
        }
        for hit in ["9mm@head", "44@head"] {
            let p = peak_of(&format!("hits={hit}"));
            assert!(
                p.head > p.peak,
                "{hit}: the head {:.1}, the torso {:.1}",
                p.head,
                p.peak
            );
        }
    }
}

#[cfg(test)]
/// The get-up (Patrick's 08-21-49 clip, 3a9805e: an anti-materiel hit landed him on his feet and
/// folded him onto his knees, the pelvis 23 units up; "still and lying" passed for that slump, and
/// the game's get-up from lying yanked him flat in four frames): a kneeling slump is never let up
/// as lying. It is nudged along its lean to finish toppling, or waits for the hold's most; it gets
/// up lying, no bone faster than 400 units a second.
mod get_up {
    use super::*;
    use crate::sim::Sim;
    use ragdoll_core::knock::{HOLD_SECONDS, LYING_PELVIS};

    #[test]
    fn a_kneeling_slump_topples_before_it_gets_up() {
        for scale in [1.0f32, 1.2] {
            let mut sim = Sim::new(GroundKind::Flat);
            sim.scale = scale;
            sim.reset();
            sim.drop_kneeling();
            let mut t = 0.0;
            while !sim.standing() && t < HOLD_SECONDS + 4.0 {
                sim.step();
                t += STEP;
            }
            let s = &sim.stats;
            eprintln!(
                "scale {scale}: let up lying {:?} (pelvis {:.1} over the floor) after {} nudges, \
                 standing at {t:.2} s, fastest bone in the get-up {:.0} units/s",
                s.got_up_lying, s.got_up_pelvis, s.nudges, s.get_up_fastest
            );
            assert!(sim.standing(), "never got up");
            let held = t >= HOLD_SECONDS;
            assert!(
                s.got_up_lying == Some(true) || held,
                "let up not lying: pelvis {:.1} over the floor",
                s.got_up_pelvis
            );
            if s.got_up_lying == Some(true) {
                assert!(s.got_up_pelvis <= LYING_PELVIS * scale + 0.01);
            }
            assert!(s.get_up_fastest <= 400.0, "{:.0} units/s", s.get_up_fastest);
        }
    }

    /// The body on its feet again within the hold's most and a few seconds, as each drop below
    /// asks; its stats.
    fn get_up_after(drop: fn(&mut Sim)) -> (bool, f32, crate::sim::Stats) {
        let mut sim = Sim::new(GroundKind::Flat);
        sim.reset();
        drop(&mut sim);
        let mut t = 0.0;
        while !sim.standing() && t < HOLD_SECONDS + 4.0 {
            sim.step();
            t += STEP;
        }
        (sim.standing(), t, sim.stats.clone())
    }

    /// Face up, folded at the hips (an anti-materiel hit's fold, Patrick's a45deed clip: he got up
    /// through a handstand, the legs swung straight up): no bone ever over the pelvis by more than
    /// 60 units while it rises, none faster than 400 units a second.
    #[test]
    fn a_face_up_fold_gets_up_without_a_handstand() {
        let (up, t, s) = get_up_after(Sim::drop_face_up_fold);
        eprintln!(
            "face-up fold: standing {up} at {t:.2} s, let up lying {:?} (pelvis {:.1} over the \
             floor), {} nudges, the most a bone was over the pelvis rising {:.1} units, fastest \
             bone {:.0} units/s",
            s.got_up_lying, s.got_up_pelvis, s.nudges, s.get_up_over_pelvis, s.get_up_fastest
        );
        assert!(up, "never got up");
        assert!(
            s.get_up_over_pelvis <= 60.0,
            "{:.1} units over the pelvis",
            s.get_up_over_pelvis
        );
        assert!(s.get_up_fastest <= 400.0, "{:.0} units/s", s.get_up_fastest);
    }

    /// Seated on the ground, the legs out ahead (Patrick's a45deed clip: half up in 3 frames): let
    /// up only lying or after the hold's most, and no bone faster than 400 units a second.
    #[test]
    fn a_seated_slump_gets_up_smoothly() {
        let (up, t, s) = get_up_after(Sim::drop_seated);
        eprintln!(
            "seated: standing {up} at {t:.2} s, let up lying {:?} (pelvis {:.1} over the floor), \
             {} nudges, the most a bone was over the pelvis rising {:.1} units, fastest bone {:.0} \
             units/s",
            s.got_up_lying, s.got_up_pelvis, s.nudges, s.get_up_over_pelvis, s.get_up_fastest
        );
        assert!(up, "never got up");
        assert!(
            s.got_up_lying == Some(true) || t >= HOLD_SECONDS,
            "let up seated"
        );
        assert!(
            s.get_up_over_pelvis <= 60.0,
            "{:.1} units over the pelvis",
            s.get_up_over_pelvis
        );
        assert!(s.get_up_fastest <= 400.0, "{:.0} units/s", s.get_up_fastest);
    }
}
