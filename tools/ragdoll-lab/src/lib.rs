//! nv-rs ragdoll lab: the game's active ragdoll (flinch, balance steps, wall braces, falls,
//! knockdowns) on a capsule mannequin in the browser, on Rapier, for quick tuning. The controller
//! is the game's own (`crates/ragdoll-core`); `sim` documents what is the game's and what is the
//! lab's. `www/` is the page; `README.md` says how to build and use it.

pub mod convert;
pub mod mannequin;
pub mod render;
pub mod scenario;
pub mod sim;
pub mod weapons;
pub mod world;

use ragdoll_core::balance::Limbs;
use ragdoll_core::flinch::*;
use ragdoll_core::knock::Body;
use ragdoll_core::rounds::Load;
use ragdoll_core::tuning::Tuning;
use render::Camera;
use sim::{Sim, STEP};
use wasm_bindgen::prelude::*;
use weapons::Weapon;
use world::GroundKind;

/// The page's handle on the lab.
#[wasm_bindgen]
pub struct Lab {
    sim: Sim,
    camera: Camera,
    marked: Option<usize>,
    arrow: Option<(V3, V3)>,
    carry: f32,
    log_seen: usize,
    /// The stance chosen on the page (a reset puts an interrupted attack back).
    armed: bool,
    attacking: bool,
}

impl Default for Lab {
    fn default() -> Self {
        Self::new()
    }
}

fn json_str(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

#[wasm_bindgen]
impl Lab {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Lab {
        Lab {
            sim: Sim::new(GroundKind::Flat),
            camera: Camera::default(),
            marked: None,
            arrow: None,
            carry: 0.0,
            log_seen: 0,
            armed: false,
            attacking: false,
        }
    }

    // --- Tuning. -----------------------------------------------------------------------------

    /// Every tuning value's description, as JSON: name, group, min, max, step, ini key, doc,
    /// the game's value and the value now.
    pub fn fields_json(&self) -> String {
        let rows: Vec<String> = Tuning::fields()
            .iter()
            .map(|f| {
                format!(
                    "{{\"name\":{},\"group\":{},\"min\":{},\"max\":{},\"step\":{},\"ini\":{},\
                     \"doc\":{},\"game\":{},\"value\":{}}}",
                    json_str(&f.name),
                    json_str(f.group),
                    f.min,
                    f.max,
                    f.step,
                    f.ini.map_or("null".into(), json_str),
                    json_str(f.doc),
                    Tuning::GAME.get(&f.name).unwrap_or(0.0),
                    self.sim.tune.get(&f.name).unwrap_or(0.0)
                )
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    /// A tuning value set (held within its range); answers the value now.
    pub fn set_value(&mut self, name: &str, value: f32) -> f32 {
        let mut t = self.sim.tune;
        t.set(name, value);
        self.sim.set_tuning(t);
        t.get(name).unwrap_or(f32::NAN)
    }

    pub fn value(&self, name: &str) -> f32 {
        self.sim.tune.get(name).unwrap_or(f32::NAN)
    }

    /// Every value back to the game's.
    pub fn game_values(&mut self) {
        self.sim.set_tuning(Tuning::GAME);
    }

    pub fn export_ini(&self) -> String {
        self.sim.tune.to_ini()
    }

    /// Values read from ini text; answers the keys not understood (comma list).
    pub fn import_ini(&mut self, text: &str) -> String {
        let (t, unknown) = self.sim.tune.from_ini(text);
        self.sim.set_tuning(t);
        unknown.join(", ")
    }

    // --- The scene. --------------------------------------------------------------------------

    pub fn reset(&mut self) {
        self.sim.armed = self.armed;
        self.sim.attacking = self.attacking;
        self.sim.reset();
        self.marked = None;
        self.arrow = None;
    }

    pub fn set_ground(&mut self, name: &str) -> bool {
        match GroundKind::named(name) {
            Some(g) => {
                self.sim.set_ground(g);
                self.marked = None;
                self.arrow = None;
                true
            }
            None => false,
        }
    }

    pub fn ground(&self) -> String {
        self.sim.ground.kind.name().into()
    }

    /// The body: `human` or `mutant`, its scale, its armour's damage threshold.
    pub fn set_body(&mut self, kind: &str, scale: f32, armour: f32) {
        self.sim.body = if kind == "mutant" {
            Body::SuperMutant
        } else {
            Body::Human
        };
        self.sim.scale = if scale.is_finite() {
            scale.clamp(0.5, 2.0)
        } else {
            1.0
        };
        self.sim.armour = armour.clamp(0.0, 100.0);
        self.sim.reset();
    }

    /// The body's stance: holding a weapon (aiming it), attacking (implies armed).
    pub fn set_stance(&mut self, armed: bool, attacking: bool) {
        self.armed = armed || attacking;
        self.attacking = attacking;
        self.reset();
    }

    pub fn set_limbs(&mut self, left_leg: bool, right_leg: bool, left_arm: bool, right_arm: bool) {
        self.sim.set_limbs(Limbs {
            left_leg,
            right_leg,
            left_arm,
            right_arm,
        });
        self.sim.reset();
    }

    /// The weapon: `kind` gun, melee or blast; `id` from the weapons list; `option` a gun's load
    /// or `power` for a power attack; Strength and skill for a swing; the range (m) for a gun.
    pub fn set_weapon(
        &mut self,
        kind: &str,
        id: &str,
        option: &str,
        strength: f32,
        skill: f32,
        range: f32,
    ) -> bool {
        let w = match kind {
            "gun" => weapons::gun(id).map(|gun| Weapon::Gun {
                gun,
                load: weapons::load(option).unwrap_or(Load::Standard),
            }),
            "melee" => weapons::melee(id).map(|melee| Weapon::Melee {
                melee,
                strength: strength.clamp(1.0, 10.0),
                skill: skill.clamp(0.0, 100.0),
                power: option == "power",
            }),
            "blast" => weapons::blast(id).map(|blast| Weapon::Blast { blast }),
            _ => None,
        };
        self.sim.range = range.clamp(0.5, 200.0);
        match w {
            Some(w) => {
                self.sim.weapon = w;
                true
            }
            None => false,
        }
    }

    pub fn weapons_json() -> String {
        weapons::json()
    }

    pub fn grounds_json() -> String {
        let g: Vec<String> = world::GROUNDS.iter().map(|g| json_str(g.0)).collect();
        format!("[{}]", g.join(","))
    }

    // --- Time. -------------------------------------------------------------------------------

    /// `seconds` of simulated time (fixed 60 Hz steps; the rest carried to the next call).
    pub fn advance(&mut self, seconds: f32) {
        if !seconds.is_finite() || seconds <= 0.0 {
            return;
        }
        self.carry += seconds.min(0.25);
        while self.carry >= STEP {
            self.carry -= STEP;
            self.sim.step();
        }
    }

    /// One step (1/60 s).
    pub fn frame_step(&mut self) {
        self.sim.step();
    }

    pub fn time(&self) -> f64 {
        self.sim.t
    }

    // --- The view and the clicks. ------------------------------------------------------------

    pub fn set_camera(&mut self, yaw: f32, pitch: f32, distance: f32, width: f32, height: f32) {
        let c = &mut self.camera;
        c.yaw = yaw;
        c.pitch = pitch.clamp(-0.2, 1.4);
        c.distance = distance.clamp(80.0, 1500.0);
        c.width = width.max(1.0);
        c.height = height.max(1.0);
        // Follow the body.
        let p = if self.sim.in_ragdoll() {
            self.sim.posed[mannequin::PELVIS].1
        } else {
            self.sim.pos
        };
        c.target = [p[0], p[1], p[2] + 60.0 * self.sim.scale];
    }

    /// The scene as the draw list (`render` documents it).
    pub fn draw(&self) -> Vec<f32> {
        render::draw(&self.sim, &self.camera, self.marked, self.arrow)
    }

    /// A click at a pixel: the weapon fired there. A gun or a swing needs the body under the
    /// pixel; a blast goes off where the ray meets the ground (or a metre short of the body).
    /// Answers what happened.
    pub fn click(&mut self, sx: f32, sy: f32) -> String {
        let (from, dir) = self.camera.ray(sx, sy);
        match self.sim.weapon {
            Weapon::Blast { .. } => {
                let centre = match render::pick_body(&self.sim, from, dir) {
                    Some((_, at)) => sub(at, scale(dir, UNITS_PER_METRE)),
                    None => match render::pick_ground(&self.sim, from, dir) {
                        Some(g) => add(g, [0.0, 0.0, 10.0]),
                        None => return "missed: nothing under the pointer".into(),
                    },
                };
                self.sim.fire(centre, dir, "blast", &render::pick_body);
                self.arrow = None;
                "blast".into()
            }
            _ => match render::pick_body(&self.sim, from, dir) {
                Some((bone, at)) => {
                    self.marked = Some(bone);
                    self.arrow = Some((at, dir));
                    self.sim
                        .fire(at, dir, mannequin::short_name(bone), &render::pick_body);
                    mannequin::short_name(bone).into()
                }
                None => "missed".into(),
            },
        }
    }

    /// The bone under a pixel ("" for none), for the pointer's label.
    pub fn hover(&self, sx: f32, sy: f32) -> String {
        let (from, dir) = self.camera.ray(sx, sy);
        render::pick_body(&self.sim, from, dir)
            .map_or(String::new(), |(b, _)| BONES[b].0.to_string())
    }

    // --- Reports. ----------------------------------------------------------------------------

    /// The log lines since the last call.
    pub fn take_log(&mut self) -> String {
        if self.log_seen > self.sim.log.len() {
            self.log_seen = 0;
        }
        let out = self.sim.log[self.log_seen..].join("\n");
        self.log_seen = self.sim.log.len();
        out
    }

    /// What has happened since the reset, as JSON.
    pub fn status_json(&self) -> String {
        let st = &self.sim.stats;
        let p = if self.sim.in_ragdoll() {
            self.sim.posed[mannequin::PELVIS].1
        } else {
            self.sim.pos
        };
        let distance =
            length(sub([p[0], p[1], 0.0], [st.start[0], st.start[1], 0.0])) / UNITS_PER_METRE;
        let state = match self.sim.state {
            sim::State::Standing if self.sim.rig.fall.is_some() => "going over",
            sim::State::Standing if self.sim.rig.balance.is_some() => "stepping",
            sim::State::Standing if self.sim.rig.brace.is_some() => "bracing",
            sim::State::Standing if self.sim.rig.active => "flinching",
            sim::State::Standing => "standing",
            sim::State::Ragdoll { .. } => "ragdoll",
            sim::State::GettingUp { .. } => "getting up",
        };
        let stack = self.sim.stack.map_or(0.0, |s| {
            ragdoll_core::knock::decayed(
                s.total,
                (self.sim.t - s.at) as f32,
                self.sim.tune.impact_stacking.max(1.0) / 1000.0,
            )
        });
        format!(
            "{{\"state\":{},\"time\":{:.2},\"hits\":{},\"fell\":{},\"fall\":{},\"steps\":{},\
             \"braced\":{},\"distance_m\":{distance:.3},\"recovery_s\":{},\"peak_speed\":{:.3},\
             \"topple\":{:.3},\"mass\":{:.1},\"stack\":{stack:.2},\"armed\":{},\"attacking\":{},\
             \"interrupted\":{},\"aim_drift\":{:.2},\"arm_spin\":{:.2}}}",
            json_str(state),
            self.sim.t,
            st.hits,
            st.fell,
            st.fall.as_deref().map_or("null".into(), json_str),
            st.steps,
            st.braced,
            match (st.recovered_at, st.first_hit) {
                (Some(r), Some(f)) => format!("{:.2}", r - f),
                _ => "null".into(),
            },
            st.peak_speed,
            self.sim.topple(),
            self.sim.mass(),
            self.sim.armed,
            self.sim.attacking,
            st.interrupted,
            st.aim_drift,
            st.arm_spin.max(st.ragdoll_arm_spin),
        )
    }

    /// The built-in scenarios, as JSON: name, text, expectations.
    pub fn scenarios_json() -> String {
        let rows: Vec<String> = scenario::BUILT_IN
            .iter()
            .map(|(n, t, e)| {
                format!(
                    "{{\"name\":{},\"text\":{},\"expect\":{}}}",
                    json_str(n),
                    json_str(t),
                    json_str(e)
                )
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    /// Runs a scenario to its end (a built-in's name, or the scenario text; the tuning now under
    /// the scenario's own changes) and answers its report as JSON (`{"error": ...}` when the text
    /// is wrong). The scene is left as the run ended.
    pub fn run_scenario(&mut self, spec: &str) -> String {
        let parsed = match scenario::Spec::built_in(spec) {
            Some(s) => Ok(s),
            None => scenario::Spec::parse(spec),
        };
        let base = self.sim.tune;
        let out = match parsed.and_then(|s| scenario::run(&mut self.sim, &s, base)) {
            Ok(r) => r.json(),
            Err(e) => format!("{{\"error\":{}}}", json_str(&e)),
        };
        // The lab's own tuning back (a scenario's changes were for its run).
        self.sim.set_tuning(base);
        self.log_seen = 0;
        out
    }

    /// Every built-in scenario run, as a JSON list of reports.
    pub fn run_all(&mut self) -> String {
        let names: Vec<&str> = scenario::BUILT_IN.iter().map(|b| b.0).collect();
        let reports: Vec<String> = names.iter().map(|n| self.run_scenario(n)).collect();
        format!("[{}]", reports.join(","))
    }
}
