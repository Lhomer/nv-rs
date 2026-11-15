//! Between the game's units and rotations (game units, row-major 3 x 3 as `NiMatrix3`) and
//! Rapier's (metres, quaternions).

use ragdoll_core::flinch::{M3, UNITS_PER_METRE, V3};
use rapier3d::math::{Mat3, Rot3, Vec3};

pub fn to_m(v: V3) -> Vec3 {
    Vec3::new(
        v[0] / UNITS_PER_METRE,
        v[1] / UNITS_PER_METRE,
        v[2] / UNITS_PER_METRE,
    )
}

pub fn from_m(v: Vec3) -> V3 {
    [
        v.x * UNITS_PER_METRE,
        v.y * UNITS_PER_METRE,
        v.z * UNITS_PER_METRE,
    ]
}

/// A velocity (or any vector) without the unit change.
pub fn vec(v: V3) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}

pub fn quat_of(m: &M3) -> Rot3 {
    let mat = Mat3::from_cols(
        Vec3::new(m[0], m[3], m[6]),
        Vec3::new(m[1], m[4], m[7]),
        Vec3::new(m[2], m[5], m[8]),
    );
    Rot3::from_mat3(&mat).normalize()
}

pub fn m3_of(q: Rot3) -> M3 {
    let m = Mat3::from_quat(q);
    [
        m.x_axis.x, m.y_axis.x, m.z_axis.x, m.x_axis.y, m.y_axis.y, m.z_axis.y, m.x_axis.z,
        m.y_axis.z, m.z_axis.z,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ragdoll_core::flinch::{apply_m, rotation};

    #[test]
    fn rotations_go_there_and_back() {
        let r = rotation([0.3, -0.2, 0.9]);
        let back = m3_of(quat_of(&r));
        for (a, b) in r.iter().zip(back.iter()) {
            assert!((a - b).abs() < 1e-5, "{r:?} {back:?}");
        }
        // The same rotation of a vector both ways.
        let v = [1.0, 2.0, 3.0];
        let a = apply_m(&r, v);
        let b = quat_of(&r) * Vec3::new(1.0, 2.0, 3.0);
        assert!((a[0] - b.x).abs() < 1e-5 && (a[2] - b.z).abs() < 1e-5);
        assert_eq!(from_m(to_m([70.0, 0.0, -35.0])), [70.0, 0.0, -35.0]);
    }
}
