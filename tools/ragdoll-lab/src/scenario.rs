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
/// `interrupted` (its attack), `attacking` (it was not)).
pub const BUILT_IN: [(&str, &str, &str); 20] = [
    (
        "pistol-chest",
        "ground=flat&hits=9mm@chest",
        "upright,steps=0,flinched,armspin<=6",
    ),
    (
        "magnum-chest",
        "ground=flat&hits=44@chest",
        "upright,flinched,armspin<=6",
    ),
    (
        "rifle-burst",
        "ground=flat&hits=556@chest*3~0.1",
        "upright,armspin<=6",
    ),
    (
        "hunting-rifle-side",
        "ground=flat&hits=308@chest/90",
        "upright",
    ),
    ("leg-shot", "ground=flat&hits=44@lthigh", "upright"),
    (
        "shotgun-close",
        "ground=flat&range=4&hits=12ga@chest",
        "fell,ragdollarmspin<=8.5",
    ),
    (
        "anti-materiel",
        "ground=flat&hits=50mg@chest",
        "fell,ragdollarmspin<=8.5",
    ),
    ("fists", "ground=flat&hits=fists@chest", "upright,steps=0"),
    (
        "sledge-power",
        "ground=flat&hits=sledge:power@chest",
        "fell",
    ),
    (
        "frag-2m",
        "ground=flat&hits=frag@2",
        "fell,ragdollarmspin<=8.5",
    ),
    (
        "wall-brace",
        "ground=wall&hits=4570@chest",
        "upright,braced,armspin<=6",
    ),
    (
        "ledge",
        "ground=ledge&hits=4570@chest",
        "fell,fall:stepped off a ledge,armspin<=6,ragdollarmspin<=8.5",
    ),
    (
        "slope-down",
        "ground=slope&hits=4570@chest",
        "upright,steps>=1",
    ),
    (
        "stairs-down",
        "ground=stairs&hits=4570@chest",
        "upright,steps>=1",
    ),
    (
        "uneven",
        "ground=uneven&hits=4570@chest",
        "upright,steps>=1",
    ),
    ("one-leg", "ground=flat&limbs=0111&hits=4570@chest", "fell"),
    (
        "armed-pistol",
        "ground=flat&armed=1&hits=9mm@chest",
        "upright,flinched,aim<=6,steps=0",
    ),
    (
        "attacking-pistol",
        "ground=flat&attacking=1&hits=9mm@chest*2~0.2",
        "upright,flinched,aim<=6,attacking",
    ),
    (
        "attacking-magnum",
        "ground=flat&attacking=1&hits=44@chest",
        "upright,flinched,interrupted,steps>=1",
    ),
    (
        "armed-shotgun",
        "ground=flat&armed=1&range=4&hits=12ga@chest",
        "fell",
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
                "armed" => spec.armed = value != "0",
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
             \"aim_drift_deg\":{:.2},\"interrupted\":{},\
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
                e if e.starts_with("aim<=") => {
                    e[5..].parse::<f32>().is_ok_and(|d| report.aim_drift <= d)
                }
                e if e.starts_with("armspin<=") => {
                    e[9..].parse::<f32>().is_ok_and(|w| report.arm_spin <= w)
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
        }
        assert!(failed.is_empty(), "{failed:#?}");
    }
}
