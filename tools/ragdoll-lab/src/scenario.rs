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
        "ground=wall&hits=4570@chest",
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
    // braced on at their height, folded over, tripped on; and sustained fire escalating.
    (
        "railing-brace",
        "ground=railing&hits=4570@chest",
        "upright,braced:waist,armspin<=6.5",
    ),
    (
        "half-wall-fold",
        "ground=halfwall&hits=50mg@chest",
        "fell,fall:folded over a waist-high obstacle,ragdollarmspin<=8.5,armspin<=6.5",
    ),
    (
        "table-edge-brace",
        "ground=table&hits=4570@chest",
        "upright,braced:waist,armspin<=6.5",
    ),
    (
        "curb-trip",
        "ground=curb&hits=4570@chest",
        "tripped,fell,fall:tripped over a low obstacle,armspin<=6.5",
    ),
    (
        "gap-railing",
        "ground=gaprailing&hits=4570@chest",
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
        "ground=wall&armed=pistol&hits=4570@chest",
        "upright,braced,onehand,armspin<=6.5",
    ),
    (
        "armed-rifle-wall-brace",
        "ground=wall&armed=rifle&hits=4570@chest",
        "upright,braced,onehand,armspin<=6.5",
    ),
    (
        "armed-pistol-railing-brace",
        "ground=railing&armed=pistol&hits=4570@chest",
        "upright,braced:waist,onehand,armspin<=6.5",
    ),
    (
        "armed-rifle-railing-brace",
        "ground=railing&armed=rifle&hits=4570@chest",
        "upright,braced:waist,onehand,armspin<=6.5",
    ),
    // The support hand goes back to the rifle's grip smoothly after the brace (Patrick,
    // 2026-11-15, e2e5531 reel: it wobbled as it regripped).
    (
        "rifle-wall-regrip",
        "ground=wall&armed=rifle&hits=4570@chest/10",
        "upright,braced,onehand,regripped,regripover<=0.5,regripwobble<=0,armspin<=6.5",
    ),
    (
        "rifle-railing-regrip",
        "ground=railing&armed=rifle&hits=4570@chest/-10",
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
    let side = [yaw.sin(), yaw.cos(), 0.0];
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
                            "name={ground}-{armed}-{gun}-{from}&ground={ground}&armed={armed}&hits={gun}@chest/{from}"
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

    /// The arm speed cap while standing (the scenes' `armspin<=6.5`), and how far an armed body's
    /// weapon arm may turn off its aim while its free hand braces (degrees).
    const MOST_ARM_SPIN: f32 = 6.5;
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
                write_path(spec, rewrites);
            }
        }
    }

    /// The write path on the lab's shown poses for `spec`: each frame the bones' locals are worked
    /// out over the animation's (`pose::local_for`) and written; then `rewrites` times something
    /// (a pass that runs after the write: the game's, before nv-rs stopped writing through it)
    /// reads them and writes them back a hair off; and some frames the animation does not write at
    /// all. The world pose the written locals make is the shown pose, every frame, and nothing
    /// adds up.
    fn write_path(name: &str, rewrites: usize) {
        let mut sim = Sim::new(GroundKind::Flat);
        let spec = Spec::parse(name).unwrap();
        prepare(&mut sim, &spec, Tuning::GAME).unwrap();
        // The bones' locals in the "game": start as the animation's.
        let anim_locals = |sim: &Sim| -> Vec<(M3, V3)> {
            let anim = sim.anim();
            (0..COUNT)
                .map(|b| match PARENT[b] {
                    Some(p) => (
                        mul(&transpose(&anim[p].0), &anim[b].0),
                        apply_m(&transpose(&anim[p].0), sub(anim[b].1, anim[p].1)),
                    ),
                    None => (anim[b].0, sub(anim[b].1, sim.pos)),
                })
                .collect()
        };
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
            // The animation writes the locals afresh on most frames, not on every one.
            if frame % 7 != 3 {
                locals = anim_locals(&sim);
            }
            // The layer: each bone's base, the animated world from the bases, the locals.
            let mut world: Vec<Xf> = Vec::with_capacity(COUNT);
            let mut parents: Vec<Xf> = Vec::with_capacity(COUNT);
            for b in 0..COUNT {
                bases[b] = Some(pose::base_of(locals[b], bases[b], written[b]));
                let up = match PARENT[b] {
                    Some(p) => world[p],
                    None => Xf {
                        r: IDENTITY,
                        t: sim.pos,
                        s: 1.0,
                    },
                };
                let base = bases[b].unwrap();
                parents.push(up);
                world.push(up.then(&Xf {
                    r: base.0,
                    t: base.1,
                    s: 1.0,
                }));
            }
            let anim = sim.anim();
            for b in 0..COUNT {
                assert!(
                    length(sub(world[b].t, anim[b].1)) < 0.5,
                    "frame {frame}: bone {b}'s animated world went off: {:?} vs {:?}",
                    world[b].t,
                    anim[b].1
                );
            }
            let shown: Vec<M3> = sim.posed.iter().map(|p| p.0).collect();
            let offsets: Vec<M3> = (0..COUNT)
                .map(|b| {
                    let own = mul(&shown[b], &transpose(&world[b].r));
                    pose::ortho(&match PARENT[b] {
                        Some(p) => mul(&mul(&world[p].r, &transpose(&shown[p])), &own),
                        None => own,
                    })
                })
                .collect();
            let shift = sub(sim.posed[mq::PELVIS].1, world[mq::PELVIS].t);
            let out: Vec<(M3, V3)> = (0..COUNT)
                .map(|b| {
                    let s = (b == mq::PELVIS).then_some(shift);
                    pose::local_for(&parents[b], bases[b].unwrap(), &offsets[b], s)
                })
                .collect();
            // Written by the layer, then `rewrites` times by a later pass, read back a hair off.
            locals.clone_from(&out);
            for _ in 0..rewrites {
                for b in 0..COUNT {
                    let mut back = out[b];
                    noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let n = (noise >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
                    back.0[(noise % 9) as usize] += n * 6e-4;
                    back.1[(noise % 3) as usize] += n * 0.01;
                    locals[b] = back;
                }
            }
            written = out.iter().map(|w| Some(*w)).collect();
            // The world the written locals make is the shown pose.
            let mut made: Vec<Xf> = Vec::with_capacity(COUNT);
            for b in 0..COUNT {
                let up = match PARENT[b] {
                    Some(p) => made[p],
                    None => Xf {
                        r: IDENTITY,
                        t: sim.pos,
                        s: 1.0,
                    },
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
                let d = length(sub(made[b].t, sim.posed[b].1));
                assert!(
                    d < 1.0,
                    "{name:?}, {rewrites} rewrites, frame {frame}: bone {b} written {d:.2} units \
                     off its shown place"
                );
                let turn = active::qlog(active::qmul(
                    active::qconj(active::qfrom_m(&made[b].r)),
                    active::qfrom_m(&sim.posed[b].0),
                ));
                assert!(
                    length(turn) < 0.02,
                    "{name:?}, {rewrites} rewrites, frame {frame}: bone {b} turned off its shown pose"
                );
            }
        }
    }
}
