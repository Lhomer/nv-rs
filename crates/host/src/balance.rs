//! Active ragdoll, stage 2: balance and stepping (Patrick, 2026-11-15), on the stage 1 rig and pose
//! layer ([`crate::active_ragdoll`]). `[nv-rs Gunplay] iBalance` (percent, 0 = off; Settings >
//! Gameplay "Balance"). **Not 1:1 with FalloutNV.exe.**
//!
//! **When it steps.** A hit below the knockdown's topple speed ([`crate::knockdown`]) gives the
//! body's centre of mass (its pelvis) a speed `v = p / mass x iBalance %` (this hit's momentum,
//! the ragdoll's mass, effective weapon stats). Its capture point, where the body would have to
//! stand to stop, lies `v sqrt(h / g)` along the push from the centre of mass ([`capture_offset`],
//! `h` the pelvis's height over the feet). Inside the support (the feet and [`FOOT_REACH`] around
//! them) nothing steps (the flinch alone); outside it, the body takes catch steps toward it
//! ([`plan`]): one or more (at most [`MAX_STEPS`]), each at most [`STEP_LENGTH`] (times its
//! scale), the feet alternating from the one on the push's side, a foot's width apart across the
//! push: bigger hits, more and longer steps.
//!
//! **How it steps.** Real leg movement in the pose layer: each foot is placed by two-bone IK
//! ([`two_bone`]: the thigh and the calf turned so the ankle reaches its target, the knee bending
//! the way it bends now, the foot kept level), the swinging foot along an arc ([`swing`]), the
//! pelvis carried over the feet and a little ahead, the torso twisted and the arms out (raised
//! from the body) while it steps, then the feet plant and the pose blends back to the animation.
//! The actor's position follows the feet only: when a foot lands, the reference and its character
//! controller are set to the feet's middle (no velocity, nothing pushed), the pelvis's offset
//! taking the move back so nothing jumps.
//!
//! **The ground** ([`classify`]): each step's target is put on the navmesh under it (slopes and
//! stairs: the foot lands on the ground, up to [`STEP_UP`] up or down). No walkable ground near
//! it is a wall or an obstacle: the step is shortened (two thirds, then a third), and with no room
//! at all the body catches itself there (stage 3 braces with its hands). Ground far lower is a
//! ledge: the body steps off and falls into its ragdoll ([`crate::knockdown::knock_down`], the
//! fall's damage as the knockdowns count it).
//!
//! **For stage 3 and dismemberment:** [`Limbs`] says which legs and arms the body still has (one
//! leg cannot step: it falls; [`Limbs::legs`]); the plan's outcome ([`Outcome`]) tells stage 3
//! when to brace or fall.

//!
//! The code lives in `crates/ragdoll-core` (`ragdoll_core::balance`), shared with the browser lab
//! `tools/ragdoll-lab`; this module re-exports it. A stepping keeps its start on a clock the caller
//! gives (here [`crate::active_ragdoll::clock`]).

pub use ragdoll_core::balance::*;
