//! Delirium's world, body and agent as safe Rust handles over its SDK (`delirium_physics`, the
//! vendored `delirium-sdk`): what New Physics asks of a Delirium body (taken from FNV's pose, shot,
//! thrown by a blast, killed, stepped, read back). Each call is the SDK's own, so its behaviours
//! (the shot's task, the blast's, falling, dying, the extra reactions) are as Delirium has them.
//!
//! Nothing here runs without the player's GTA IV: every handle refuses while Delirium's numbers
//! are locked (CI has no GTA IV, and Delirium's code cannot run there at all). Metres, seconds,
//! Z up; rotations Delirium's quaternions.

use delirium::prelude::{Quat, Transform, Vec3};
use delirium_physics as sdk;
use std::ffi::CString;

/// The SDK's last error on this thread.
fn last_error() -> String {
    let n = unsafe { sdk::nm_last_error(std::ptr::null_mut(), 0) };
    if n <= 0 {
        return "(no error text)".into();
    }
    let mut buf = vec![0u8; n as usize + 1];
    unsafe { sdk::nm_last_error(buf.as_mut_ptr().cast(), buf.len() as i32) };
    String::from_utf8_lossy(&buf)
        .trim_end_matches('\0')
        .to_string()
}

/// An SDK return: its error text when negative.
fn ok(r: i32) -> Result<i32, String> {
    if r < 0 {
        Err(last_error())
    } else {
        Ok(r)
    }
}

/// Delirium's own shot force (its SDK's `SHOT_FORCE`), every shot's in its `nm_agent_task_shot`.
pub const SHOT_FORCE: f32 = 400.0;

/// A GTA IV ped's health as Delirium's shot tasks read it: full at 200, dead under 50 (its SDK's
/// `a.dead |= health < 50.0`; its own on-fire task passes 200 for a ped unhurt).
pub const FULL_HEALTH: f32 = 200.0;
pub const DEAD_UNDER: f32 = 50.0;

/// The health Delirium's shot task is given for `left` (0 to 1 of the actor's most): never under
/// its dead line while the game has the actor alive (the game's death is the corpse's own; Patrick's
/// 17:34 log: the fraction itself was passed, under 50, so every shot made the agent dead, its
/// muscles off: a light hit sat him down, and no rise ever lifted him).
pub fn ped_health(left: f32) -> f32 {
    let alive = DEAD_UNDER + 10.0;
    let f = if left.is_finite() {
        left.clamp(0.0, 1.0)
    } else {
        1.0
    };
    alive + (FULL_HEALTH - alive) * f
}

/// Delirium steps its agents in whole 60ths of a second (its SDK banks a shorter frame).
pub const STEP: f32 = 1.0 / 60.0;

/// Refused while Delirium's numbers are locked.
fn unlocked() -> Result<(), String> {
    if delirium_data::kv::open() {
        Ok(())
    } else {
        Err("Delirium isn't unlocked (no GTA IV)".into())
    }
}

/// A Delirium collision world: the triangles of what is around one actor ([`crate::world`]).
pub struct World(*mut sdk::NmWorld);

// The handle is used by one thread at a time (New Physics' frame), as the SDK allows.
unsafe impl Send for World {}

impl World {
    pub fn new() -> Result<World, String> {
        unlocked()?;
        Ok(World(sdk::nm_world_create()))
    }

    /// Chunk `id` set to `mesh` (an empty mesh takes it out).
    pub fn set_mesh(&mut self, id: i32, mesh: &crate::world::Mesh) -> Result<(), String> {
        let materials = vec![0u8; mesh.triangles.len()];
        ok(unsafe {
            sdk::nm_world_set_mesh(
                self.0,
                id,
                mesh.positions.as_ptr().cast(),
                mesh.positions.len() as i32,
                mesh.triangles.as_ptr().cast(),
                mesh.triangles.len() as i32,
                materials.as_ptr(),
            )
        })
        .map(|_| ())
    }
}

impl Drop for World {
    fn drop(&mut self) {
        unsafe { sdk::nm_world_destroy(self.0) };
    }
}

/// A Delirium body (its build and its parts' rest frames), the SDK's.
pub struct Body(*mut sdk::NmBuild);

unsafe impl Send for Body {}
unsafe impl Sync for Body {}

impl Body {
    /// The body `build` with its parts' rest frames `frames` (fitted to FNV's skeleton, then the
    /// actor's profile's weight: [`crate::profile::Profile::kg`]).
    pub fn new(
        build: &delirium::ragdoll::Build,
        frames: &[Transform],
        kg: Option<f32>,
    ) -> Result<Body, String> {
        unlocked()?;
        let text = CString::new(build.dump(frames)).map_err(|_| "the body's text")?;
        let b = unsafe { sdk::nm_build_from_text(text.as_ptr()) };
        if b.is_null() {
            return Err(last_error());
        }
        let body = Body(b);
        if let Some(kg) = kg {
            ok(unsafe { sdk::nm_build_set_weight(body.0, kg) })?;
        }
        Ok(body)
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        unsafe { sdk::nm_build_destroy(self.0) };
    }
}

/// One part's state: its world frame and its velocities (m/s, rad/s).
#[derive(Clone, Copy, Debug)]
pub struct PartState {
    pub frame: Transform,
    pub velocity: Vec3,
    pub spin: Vec3,
}

/// A Delirium agent: one actor's body, its muscles and behaviours.
pub struct Agent(*mut sdk::NmAgent);

unsafe impl Send for Agent {}

impl Agent {
    /// An agent of `body` placed with its parts at `parts` (FNV's pose read into them:
    /// [`crate::rig::Rig::parts_at`]), moving as they do, and `profile`'s settings.
    pub fn new(
        body: &Body,
        parts: &[PartState],
        profile: &crate::profile::Profile,
    ) -> Result<Agent, String> {
        unlocked()?;
        let at = parts.first().map_or(Vec3::ZERO, |p| p.frame.translation);
        let a = unsafe { sdk::nm_agent_create(body.0, at.to_array().as_ptr(), 0.0, 0.3) };
        if a.is_null() {
            return Err(last_error());
        }
        let agent = Agent(a);
        let mut flat = Vec::with_capacity(parts.len() * 13);
        for p in parts {
            flat.extend_from_slice(&p.frame.translation.to_array());
            flat.extend_from_slice(&p.frame.rotation.to_array());
            flat.extend_from_slice(&p.velocity.to_array());
            flat.extend_from_slice(&p.spin.to_array());
        }
        ok(unsafe { sdk::nm_agent_set_part_states(agent.0, flat.as_ptr(), parts.len() as i32) })?;
        ok(unsafe { sdk::nm_agent_set_strength(agent.0, profile.strength.clamp(0.1, 10.0)) })?;
        if let Some(bits) = profile.reactions {
            ok(unsafe { sdk::nm_agent_set_reactions(agent.0, bits) })?;
        }
        Ok(agent)
    }

    /// Shot in part `part` at `at` along `dir`, with `health` left (0 to 1 of its most; Delirium's
    /// shot task: a lot left staggers, little drops it).
    pub fn shot(
        &mut self,
        world: &World,
        part: usize,
        at: Vec3,
        dir: Vec3,
        health: f32,
    ) -> Result<(), String> {
        self.shot_force(world, part, at, dir, health, SHOT_FORCE)
    }

    /// [`Agent::shot`] with its force (Delirium's shot task's: [`SHOT_FORCE`] its own).
    pub fn shot_force(
        &mut self,
        world: &World,
        part: usize,
        at: Vec3,
        dir: Vec3,
        health: f32,
        force: f32,
    ) -> Result<(), String> {
        let set = CString::new("*").map_err(|_| "set")?;
        ok(unsafe {
            sdk::nm_agent_task_shot_force(
                self.0,
                world.0,
                set.as_ptr(),
                part as i32,
                at.to_array().as_ptr(),
                dir.to_array().as_ptr(),
                ped_health(health),
                force.max(1.0),
            )
        })
        .map(|_| ())
    }

    /// Thrown by a blast at `at` (its reach `radius`, metres; its speed at the centre, m/s).
    pub fn explosion(
        &mut self,
        world: &World,
        at: Vec3,
        radius: f32,
        speed: f32,
    ) -> Result<(), String> {
        ok(unsafe {
            sdk::nm_agent_task_explosion(self.0, world.0, at.to_array().as_ptr(), radius, speed)
        })
        .map(|_| ())
    }

    /// Dead: the muscles off for good (a corpse is Delirium with its muscles off).
    pub fn kill(&mut self) -> Result<(), String> {
        // Nothing followed any more (a rise's muscles and its help on the pelvis end with it).
        self.follow(&[])?;
        ok(unsafe { sdk::nm_agent_set_dead(self.0, 1) }).map(|_| ())
    }

    /// One step of `dt` seconds in `world`.
    pub fn step(&mut self, world: &World, dt: f32) -> Result<(), String> {
        ok(unsafe { sdk::nm_agent_step(self.0, world.0, dt) }).map(|_| ())
    }

    /// Lying relaxed as Delirium's own relax has it (its own stiffness and damping): a downed
    /// body's settle.
    pub fn relax(&mut self) -> Result<(), String> {
        ok(unsafe { sdk::nm_agent_task_relax(self.0, -1.0, -1.0) }).map(|_| ())
    }

    /// Its muscles on the pose given by [`Agent::follow`] (Delirium's muscle rise: stiff muscles
    /// on every joint, the pelvis helped toward the pose's with at most `help` of its weight), or
    /// off (`on` false: its own balance again).
    pub fn rise(&mut self, world: &World, on: bool, help: f32) -> Result<(), String> {
        ok(unsafe { sdk::nm_agent_muscle_rise(self.0, world.0, on as i32, help) }).map(|_| ())
    }

    /// Each part's velocity and spin now (m/s, rad/s).
    pub fn motions(&self) -> Result<Vec<(Vec3, Vec3)>, String> {
        let mut out = Vec::with_capacity(21);
        for k in 0..21 {
            let (mut v, mut w) = ([0f32; 3], [0f32; 3]);
            ok(unsafe {
                sdk::nm_agent_get_part_velocity(self.0, k, v.as_mut_ptr(), w.as_mut_ptr())
            })?;
            out.push((Vec3::from_array(v), Vec3::from_array(w)));
        }
        Ok(out)
    }

    /// Part `part` put at `frame`, moving as `velocity` and `spin` (its joints' limits brought
    /// back onto a pose that left them).
    pub fn set_part(&mut self, part: usize, state: &PartState) -> Result<(), String> {
        ok(unsafe {
            sdk::nm_agent_set_part_state(
                self.0,
                part as i32,
                state.frame.translation.to_array().as_ptr(),
                state.frame.rotation.to_array().as_ptr(),
                state.velocity.to_array().as_ptr(),
                state.spin.to_array().as_ptr(),
            )
        })
        .map(|_| ())
    }

    /// The whole body's velocity changed by `dv` (m/s): every part alike, nothing else (no
    /// hit, no look).
    pub fn kick(&mut self, dv: Vec3) -> Result<(), String> {
        ok(unsafe { sdk::nm_agent_add_velocity(self.0, dv.to_array().as_ptr()) }).map(|_| ())
    }

    /// The pose its muscles follow: every part's world frame (`parts`, all 21), or none (empty:
    /// nothing followed).
    pub fn follow(&mut self, parts: &[Transform]) -> Result<(), String> {
        let mut flat = Vec::with_capacity(parts.len() * 7);
        for p in parts {
            flat.extend_from_slice(&p.translation.to_array());
            flat.extend_from_slice(&p.rotation.to_array());
        }
        let ptr = if flat.is_empty() {
            std::ptr::null()
        } else {
            flat.as_ptr()
        };
        ok(unsafe { sdk::nm_agent_set_incoming_transforms(self.0, ptr, parts.len() as i32) })
            .map(|_| ())
    }

    /// The parts' world frames now.
    pub fn parts(&self) -> Result<Vec<Transform>, String> {
        let mut out = vec![0f32; 21 * 7];
        let n = ok(unsafe { sdk::nm_agent_get_part_transforms(self.0, out.as_mut_ptr(), 21) })?;
        Ok(out
            .chunks(7)
            .take(n as usize)
            .map(|c| {
                Transform::from_translation(Vec3::new(c[0], c[1], c[2]))
                    .with_rotation(Quat::from_xyzw(c[3], c[4], c[5], c[6]).normalize())
            })
            .collect())
    }

    /// Whether it is keeping its balance on its feet.
    pub fn balancing(&self) -> bool {
        unsafe { sdk::nm_agent_is_balancing(self.0) == 1 }
    }

    /// Whether it lies settled and may get up after `after` seconds still (Delirium's own
    /// judgement: never a dead one).
    pub fn should_get_up(&self, after: f32) -> bool {
        unsafe { sdk::nm_agent_should_get_up(self.0, after) == 1 }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        unsafe { sdk::nm_agent_destroy(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The health given to Delirium is a ped's: full at 200, never under its dead line for a
    /// living actor, whatever share of its most is left.
    #[test]
    fn a_living_actors_health_is_never_delirium_s_dead() {
        assert_eq!(ped_health(1.0), FULL_HEALTH);
        for f in [0.0, 0.01, 0.3, 0.9, -1.0, f32::NAN] {
            assert!(ped_health(f) >= DEAD_UNDER, "{f}: {}", ped_health(f));
        }
        assert!(ped_health(0.5) > ped_health(0.2));
    }

    /// Without GTA IV nothing of Delirium is made: each handle says why.
    #[test]
    fn nothing_is_made_while_locked() {
        assert!(!delirium_data::kv::open());
        let e = World::new().err().unwrap();
        assert!(e.contains("isn't unlocked"), "{e}");
        let e = unlocked().unwrap_err();
        assert!(e.contains("no GTA IV"));
    }
}
