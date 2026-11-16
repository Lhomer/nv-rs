//! The capsule mannequin: the game's biped rig (`ragdoll_core::flinch::BONES`, the same 20 Bip01
//! bones in the same order and roles), its rest pose, its capsules, each bone's share of the body's
//! mass and its ragdoll joint's limits.
//!
//! **What is the game's and what is not.** The bone names, roles, springs, kick gains and pose-layer
//! limits are the game's own (`ragdoll_core`). The body's total mass is the knockdowns' figure for
//! the body (`ragdoll_core::knock::figures`: 80 kg for a human, the figure the game falls back to
//! before it has measured a ragdoll). The game's Havok ragdoll itself (each bone's rigid body,
//! mass and constraint limits) lives in the game's `skeleton.nif`, which is game data and is not in
//! this repository, so the bone lengths, the per-bone mass shares (standard anthropometric
//! segment fractions) and the ragdoll joints' limits (a biped ragdoll's usual cones and hinges)
//! here are stand-ins of the same layout.
//!
//! Coordinates are the game's: game units (70 a metre), Z up, the mannequin facing +Y, its left
//! side at -X.

use ragdoll_core::flinch::{Role, BONES, V3};

/// Bone indices (the order of `BONES`).
pub const PELVIS: usize = 0;
pub const SPINE: usize = 1;
pub const SPINE1: usize = 2;
pub const SPINE2: usize = 3;
pub const NECK: usize = 4;
pub const HEAD: usize = 5;
pub const L_CLAVICLE: usize = 6;
pub const R_CLAVICLE: usize = 7;
pub const L_UPPERARM: usize = 8;
pub const R_UPPERARM: usize = 9;
pub const L_FOREARM: usize = 10;
pub const R_FOREARM: usize = 11;
pub const L_THIGH: usize = 12;
pub const R_THIGH: usize = 13;
pub const L_CALF: usize = 14;
pub const R_CALF: usize = 15;
pub const L_FOOT: usize = 16;
pub const R_FOOT: usize = 17;
pub const L_HAND: usize = 18;
pub const R_HAND: usize = 19;
pub const COUNT: usize = 20;

/// Each bone's parent in the physical body (`None`: the pelvis, under the actor's root):
/// `ragdoll_core::active::physics_parents` of the game's node tree ([`NODE_PARENT`]).
pub const PARENT: [Option<usize>; COUNT] = [
    None,
    Some(PELVIS),
    Some(SPINE),
    Some(SPINE1),
    Some(SPINE2),
    Some(NECK),
    Some(SPINE2),
    Some(SPINE2),
    Some(L_CLAVICLE),
    Some(R_CLAVICLE),
    Some(L_UPPERARM),
    Some(R_UPPERARM),
    Some(PELVIS),
    Some(PELVIS),
    Some(L_THIGH),
    Some(R_THIGH),
    Some(L_CALF),
    Some(R_CALF),
    Some(L_FOREARM),
    Some(R_FOREARM),
];

/// Each bone's rig parent in the game's node tree (FNV's `skeleton.nif`): as [`PARENT`], except
/// that `Bip01 Spine` hangs from `Bip01 NonAccum`, a sibling of `Bip01 Pelvis` (the thighs are
/// under the pelvis), so it reaches no rig bone. The write follows this tree (the host's
/// `active_ragdoll`); the body follows [`PARENT`], which is
/// `ragdoll_core::active::physics_parents` of it.
pub const NODE_PARENT: [Option<usize>; COUNT] = {
    let mut t = PARENT;
    t[SPINE] = None;
    t
};

/// Each bone's joint (its node's position) at rest, scale 1, over the actor's reference.
pub const JOINT: [V3; COUNT] = [
    [0.0, 0.0, 66.0],
    [0.0, 0.0, 70.0],
    [0.0, 0.0, 78.0],
    [0.0, 0.0, 86.0],
    [0.0, 0.0, 102.0],
    [0.0, 0.5, 108.0],
    [-2.0, 0.0, 100.0],
    [2.0, 0.0, 100.0],
    [-13.0, 0.0, 100.0],
    [13.0, 0.0, 100.0],
    [-14.0, 0.5, 79.0],
    [14.0, 0.5, 79.0],
    [-7.0, 0.0, 64.0],
    [7.0, 0.0, 64.0],
    [-7.0, 1.0, 35.0],
    [7.0, 1.0, 35.0],
    [-7.0, 0.0, 6.0],
    [7.0, 0.0, 6.0],
    [-15.0, 1.0, 59.0],
    [15.0, 1.0, 59.0],
];

/// Each bone's capsule at rest (two end points over the reference, scale 1) and radius.
pub const CAPSULE: [(V3, V3, f32); COUNT] = [
    ([-6.5, 0.0, 64.0], [6.5, 0.0, 64.0], 8.5),
    ([0.0, 0.0, 70.5], [0.0, 0.0, 77.5], 9.5),
    ([0.0, 0.0, 78.5], [0.0, 0.0, 85.5], 10.0),
    ([-6.0, 0.0, 92.0], [6.0, 0.0, 92.0], 10.0),
    ([0.0, 0.5, 102.0], [0.0, 0.5, 107.0], 4.0),
    ([0.0, 1.0, 112.0], [0.0, 1.5, 118.0], 7.0),
    ([-2.0, 0.0, 100.0], [-12.0, 0.0, 100.0], 3.5),
    ([2.0, 0.0, 100.0], [12.0, 0.0, 100.0], 3.5),
    ([-13.5, 0.0, 98.0], [-14.0, 0.5, 81.0], 4.2),
    ([13.5, 0.0, 98.0], [14.0, 0.5, 81.0], 4.2),
    ([-14.0, 0.5, 77.0], [-15.0, 1.0, 61.0], 3.6),
    ([14.0, 0.5, 77.0], [15.0, 1.0, 61.0], 3.6),
    ([-7.0, 0.0, 61.0], [-7.0, 1.0, 38.0], 6.2),
    ([7.0, 0.0, 61.0], [7.0, 1.0, 38.0], 6.2),
    ([-7.0, 1.0, 33.0], [-7.0, 0.0, 9.0], 4.8),
    ([7.0, 1.0, 33.0], [7.0, 0.0, 9.0], 4.8),
    ([-7.0, 0.0, 4.0], [-7.0, 9.0, 2.5], 3.0),
    ([7.0, 0.0, 4.0], [7.0, 9.0, 2.5], 3.0),
    ([-15.0, 1.0, 57.5], [-15.0, 1.5, 52.0], 3.2),
    ([15.0, 1.0, 57.5], [15.0, 1.5, 52.0], 3.2),
];

/// Each bone's share of the body's mass (segment fractions; they sum to 1).
pub const MASS_SHARE: [f32; COUNT] = [
    0.142, 0.0695, 0.0695, 0.206, 0.010, 0.071, 0.005, 0.005, 0.028, 0.028, 0.016, 0.016, 0.100,
    0.100, 0.0465, 0.0465, 0.0145, 0.0145, 0.006, 0.006,
];

/// Where a bone's body reaches at rest (scale 1): the next joint down its chain, or the far end of
/// its capsule (the active ragdoll's segments, `ragdoll_core::active`).
pub fn end(bone: usize) -> V3 {
    match bone {
        PELVIS => JOINT[SPINE],
        SPINE => JOINT[SPINE1],
        SPINE1 => JOINT[SPINE2],
        SPINE2 => JOINT[NECK],
        NECK => JOINT[HEAD],
        L_CLAVICLE => JOINT[L_UPPERARM],
        R_CLAVICLE => JOINT[R_UPPERARM],
        L_UPPERARM => JOINT[L_FOREARM],
        R_UPPERARM => JOINT[R_FOREARM],
        L_FOREARM => JOINT[L_HAND],
        R_FOREARM => JOINT[R_HAND],
        L_THIGH => JOINT[L_CALF],
        R_THIGH => JOINT[R_CALF],
        L_CALF => JOINT[L_FOOT],
        R_CALF => JOINT[R_FOOT],
        _ => CAPSULE[bone].1,
    }
}

/// The ragdoll joint's limits (radians) about the world axes at rest: X across the body (a knee
/// or an elbow bends about it), Y forward, Z up. `[min, max]` per axis.
pub fn ragdoll_limits(bone: usize) -> [[f32; 2]; 3] {
    let s = |x: f32, y: f32, z: f32| [[-x, x], [-y, y], [-z, z]];
    match bone {
        SPINE | SPINE1 => s(0.3, 0.25, 0.25),
        SPINE2 => s(0.35, 0.3, 0.3),
        NECK => s(0.5, 0.4, 0.6),
        HEAD => s(0.5, 0.4, 0.7),
        L_CLAVICLE | R_CLAVICLE => s(0.2, 0.2, 0.25),
        L_UPPERARM | R_UPPERARM => s(1.4, 1.3, 0.9),
        // An elbow bends forward only (about X; its stop at straight), a little twist.
        L_FOREARM | R_FOREARM => [[0.0, 2.4], [-0.1, 0.1], [-0.6, 0.6]],
        // A hip: forward far, back a little; out and in; twist.
        L_THIGH | R_THIGH => [[-0.4, 1.7], [-0.6, 0.6], [-0.5, 0.5]],
        // A knee bends back only (its stop at straight).
        L_CALF | R_CALF => [[-2.3, 0.0], [-0.05, 0.05], [-0.15, 0.15]],
        L_FOOT | R_FOOT => s(0.6, 0.3, 0.3),
        L_HAND | R_HAND => s(0.8, 0.4, 0.5),
        _ => s(0.0, 0.0, 0.0),
    }
}

/// A bone's role and side (0 left, 1 right, 2 the middle), from its name as the game reads it.
pub fn role_side(bone: usize) -> (Role, u8) {
    let (name, role) = BONES[bone];
    let side = if name.contains(" L ") {
        0
    } else if name.contains(" R ") {
        1
    } else {
        2
    };
    (role, side)
}

/// The bone of a role on a side (side 2: any).
pub fn bone_of(role: Role, side: u8) -> Option<usize> {
    (0..COUNT).find(|&b| {
        let (r, s) = role_side(b);
        r == role && (side == 2 || s == side)
    })
}

/// A short name for a bone (the lab's labels and the scenarios' `@part`).
pub fn short_name(bone: usize) -> &'static str {
    [
        "pelvis",
        "belly",
        "spine1",
        "chest",
        "neck",
        "head",
        "lclavicle",
        "rclavicle",
        "lupperarm",
        "rupperarm",
        "lforearm",
        "rforearm",
        "lthigh",
        "rthigh",
        "lcalf",
        "rcalf",
        "lfoot",
        "rfoot",
        "lhand",
        "rhand",
    ][bone]
}

/// A bone by its short name or its Bip01 name (any case).
pub fn bone_named(name: &str) -> Option<usize> {
    let n = name.trim().to_ascii_lowercase();
    (0..COUNT).find(|&b| short_name(b) == n || BONES[b].0.to_ascii_lowercase() == n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rig_is_the_games() {
        assert_eq!(COUNT, BONES.len());
        let total: f32 = MASS_SHARE.iter().sum();
        assert!((total - 1.0).abs() < 1e-3, "{total}");
        for (b, parent) in PARENT.iter().enumerate() {
            if let Some(p) = parent {
                assert!(*p < b, "parents come first");
            }
        }
        assert_eq!(role_side(L_THIGH), (Role::Thigh, 0));
        assert_eq!(role_side(R_HAND), (Role::Hand, 1));
        assert_eq!(role_side(SPINE2), (Role::Spine, 2));
        assert_eq!(bone_of(Role::Foot, 1), Some(R_FOOT));
        assert_eq!(bone_named("chest"), Some(SPINE2));
        assert_eq!(bone_named("Bip01 L Calf"), Some(L_CALF));
    }
}
