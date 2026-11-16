//! The pure parts of the living-body physics (Patrick, 2026-11-15): no game memory, no engine, no
//! clock, so that the host (`crates/host`) and the browser lab (`tools/ragdoll-lab`) run the same
//! code and a tuning tried in the lab carries over to the game.
//!
//! - [`active`]: the active ragdoll's physical body (PD motors on every joint tracking the
//!   animation, the hit travelling through the chain, balance from the motors and the steps).
//! - [`flinch`]: the active ragdoll's bone roles, springs, kick spread and stage 3 timings
//!   (`crate::active_ragdoll` in the host re-exports it).
//! - [`balance`]: stage 2's capture point, catch-step planning, ground classes, two-bone IK and the
//!   stepping state (the host's `crate::balance`).
//! - [`knock`]: the knockdowns' bodies, momentum transfer, topple speed, impact stacking and launch
//!   (the host's `crate::knockdown` and `crate::hit_force`).
//! - [`pose`]: the pose layer's writes made so they cannot run away (absolute locals over the
//!   animation's own, rotations kept orthonormal, sanity checks).
//! - [`rounds`]: the calibres, ammo loads and the share of a round's momentum left in a body (the
//!   host's `crate::ballistics`).
//! - [`tuning`]: every constant above (and the game's ini settings that feed them) by name, with a
//!   process-wide override the lab sets from its sliders. The game never sets it, so in the game
//!   [`tuning::get`] is always [`tuning::Tuning::GAME`], the constants as written.

pub mod active;
pub mod balance;
pub mod brace;
pub mod condition;
pub mod flinch;
pub mod knock;
pub mod obstacle;
pub mod one_hand;
pub mod pose;
pub mod rounds;
pub mod tuning;

/// Game units a metre (the engine's 70).
pub const UNITS_PER_METRE: f32 = 70.0;
