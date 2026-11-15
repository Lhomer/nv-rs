//! The pose layer's writes, made so they cannot run away (Patrick, 2026-11-15: an armed Powder
//! Ganger stuttered, stretched and grew, then the game froze).
//!
//! What went wrong in f36ddda: the layer took a bone's local transform as the animation's pose
//! whenever it differed from what the layer last wrote. The game's ragdoll-controller pass reads
//! the pose and writes it back (through Havok's transforms, and with the grab IK on the hands),
//! so a written bone came back a hair different, was taken for a fresh animated pose, and got the
//! offset again: every frame, on top of the last. And the animated pose in the world was read
//! from the bones' world transforms, which already held the last write: the body chased its own
//! output.
//!
//! Now ([`base_of`], [`local_for`], [`Xf`]): a bone that still holds (within [`ROT_TOL`] and
//! [`POS_TOL`]) what was written is under its remembered animated pose; every rotation read or
//! written is made orthonormal ([`ortho`], so no scale or shear ever gets in); the world pose
//! under the layer is worked out from the animation's own locals ([`Xf::then`]), never read back;
//! what is written is the local the shown pose needs (absolute, the same however often it is
//! written); and the offsets are checked ([`sane`]).

use crate::flinch::{add, apply_m, length, mul, scale, sub, transpose, IDENTITY, M3, V3};

/// How close a bone's local must be to what was written to count as still holding it: rotation
/// entries, translation (game units).
pub const ROT_TOL: f32 = 2e-3;
pub const POS_TOL: f32 = 0.05;
/// The most the pelvis is moved off its animation (game units), and how far a bone's animated
/// joint may be from the actor's root (game units) before the pose is not believed.
pub const MAX_PELVIS_OFFSET: f32 = 80.0;
pub const MAX_REACH: f32 = 600.0;

/// A transform: rotation (row-major, as `NiMatrix3`), translation, uniform scale (as
/// `NiTransform`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub r: M3,
    pub t: V3,
    pub s: f32,
}

impl Xf {
    pub const IDENTITY: Xf = Xf {
        r: IDENTITY,
        t: [0.0; 3],
        s: 1.0,
    };

    /// `self` (a parent's world) then `child` (its local): the child's world.
    pub fn then(&self, child: &Xf) -> Xf {
        Xf {
            r: mul(&self.r, &child.r),
            t: add(apply_m(&self.r, scale(child.t, self.s)), self.t),
            s: self.s * child.s,
        }
    }

    pub fn finite(&self) -> bool {
        self.r.iter().chain(self.t.iter()).all(|x| x.is_finite()) && self.s.is_finite()
    }
}

/// The rotation nearest `m` (through a quaternion, so the result is a rotation exactly); the
/// identity for one that is not finite or degenerate.
pub fn ortho(m: &M3) -> M3 {
    if !m.iter().all(|x| x.is_finite()) {
        return IDENTITY;
    }
    let q = crate::active::qfrom_m(m);
    if !q.iter().all(|x| x.is_finite()) {
        return IDENTITY;
    }
    crate::active::m_from_q(q)
}

/// Whether `m` is a rotation within `tol` (orthonormal rows, determinant 1).
pub fn is_rotation(m: &M3, tol: f32) -> bool {
    if !m.iter().all(|x| x.is_finite()) {
        return false;
    }
    let p = mul(m, &transpose(m));
    let id = p
        .iter()
        .zip(IDENTITY.iter())
        .all(|(a, b)| (a - b).abs() <= tol);
    let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6]);
    id && (det - 1.0).abs() <= tol * 3.0
}

/// Whether the local `a` still holds `b` (what was written), within [`ROT_TOL`] and [`POS_TOL`].
pub fn holds(a: &(M3, V3), b: &(M3, V3)) -> bool {
    a.0.iter()
        .zip(b.0.iter())
        .all(|(x, y)| (x - y).abs() <= ROT_TOL)
        && length(sub(a.1, b.1)) <= POS_TOL
}

/// The animated local under the layer: the remembered pose while the bone still holds what was
/// written (nothing wrote it again, or the game's pass wrote it back), else what it holds now
/// (the animation wrote it); its rotation made orthonormal.
pub fn base_of(now: (M3, V3), pose: Option<(M3, V3)>, written: Option<(M3, V3)>) -> (M3, V3) {
    match (pose, written) {
        (Some(p), Some(w)) if holds(&now, &w) => p,
        _ => (ortho(&now.0), now.1),
    }
}

/// The local to write for a bone: under it its parent node's animated world `parent`, its
/// animated local `base`, its world offset `r` (the shown rotation over the animated one,
/// carried down the chain); for the pelvis, `shift` (world, game units) moves its translation.
/// The rotation is made orthonormal; the shift is held to [`MAX_PELVIS_OFFSET`].
pub fn local_for(parent: &Xf, base: (M3, V3), r: &M3, shift: Option<V3>) -> (M3, V3) {
    let pt = transpose(&parent.r);
    let rot = ortho(&mul(&mul(&mul(&pt, r), &parent.r), &base.0));
    let t = match shift {
        Some(v) => {
            let l = length(v);
            let v = if l > MAX_PELVIS_OFFSET && l.is_finite() {
                scale(v, MAX_PELVIS_OFFSET / l)
            } else if !l.is_finite() {
                [0.0; 3]
            } else {
                v
            };
            add(base.1, scale(apply_m(&pt, v), 1.0 / parent.s.max(1e-3)))
        }
        None => base.1,
    };
    (rot, t)
}

/// Whether a pose about to be shown is believable: every world offset a rotation, the pelvis's
/// shift finite and bounded, every animated joint within [`MAX_REACH`] of `root` (the actor's
/// root's world position). `Err` says what is wrong.
pub fn sane(offsets: &[M3], shift: V3, joints: &[V3], root: V3) -> Result<(), String> {
    if let Some(i) = offsets.iter().position(|m| !is_rotation(m, 1e-2)) {
        return Err(format!("bone {i}'s offset is no rotation"));
    }
    if !(shift.iter().all(|x| x.is_finite()) && length(shift) <= MAX_PELVIS_OFFSET * 1.5) {
        return Err(format!("the pelvis's shift {shift:?} is out of bounds"));
    }
    if let Some(i) = joints
        .iter()
        .position(|j| !(j.iter().all(|x| x.is_finite()) && length(sub(*j, root)) <= MAX_REACH))
    {
        return Err(format!(
            "bone {i}'s animated joint {:?} is not believable",
            joints[i]
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flinch::rotation;

    #[test]
    fn rotations_stay_rotations() {
        let r = rotation([0.3, 0.1, -0.7]);
        assert!(is_rotation(&r, 1e-5));
        let mut skew = r;
        skew[0] *= 1.3; // a scale got in
        assert!(!is_rotation(&skew, 1e-3));
        assert!(is_rotation(&ortho(&skew), 1e-4));
        assert_eq!(ortho(&[f32::NAN; 9]), IDENTITY);
        let parent = Xf {
            r: rotation([0.0, 0.0, 1.0]),
            t: [10.0, 0.0, 0.0],
            s: 2.0,
        };
        let child = Xf {
            r: IDENTITY,
            t: [1.0, 0.0, 0.0],
            s: 1.0,
        };
        let w = parent.then(&child);
        assert!((w.t[0] - (10.0 + 2.0 * 1.0f32.cos())).abs() < 1e-4);
        assert_eq!(w.s, 2.0);
    }

    /// A written bone the game's pass writes back a hair off is still under its remembered pose;
    /// one the animation wrote is taken as it is.
    #[test]
    fn the_base_is_remembered_through_the_pass() {
        let pose = (rotation([0.1, 0.0, 0.0]), [0.0, 0.0, 5.0]);
        let written = (rotation([0.4, 0.0, 0.0]), [0.0, 0.0, 9.0]);
        let mut back = written;
        back.0[1] += 5e-4;
        back.1[2] += 0.01;
        assert_eq!(base_of(back, Some(pose), Some(written)), pose);
        let fresh = (rotation([0.2, 0.0, 0.0]), [0.0, 0.0, 5.0]);
        let b = base_of(fresh, Some(pose), Some(written));
        assert!(holds(&b, &fresh));
    }

    /// Written again and again (the pass writing back each time, a hair off, the animation not
    /// writing): the local never runs away; written twice over itself it is the same.
    #[test]
    fn the_writes_do_not_add_up() {
        let parent = Xf {
            r: rotation([0.0, 0.2, 0.0]),
            t: [0.0; 3],
            s: 1.0,
        };
        let anim = (rotation([0.1, 0.0, 0.0]), [0.0, 0.0, 5.0]);
        let r = rotation([0.0, 0.0, 0.3]);
        let shift = Some([3.0, 0.0, 0.0]);
        let mut local = anim;
        let mut pose = None;
        let mut written = None;
        let mut first = None;
        for i in 0..2000 {
            let base = base_of(local, pose, written);
            pose = Some(base);
            let w = local_for(&parent, base, &r, shift);
            written = Some(w);
            // The pass reads it and writes it back, a hair off.
            local = w;
            local.0[2] += if i % 2 == 0 { 3e-4 } else { -3e-4 };
            local.1[0] += 0.002;
            first.get_or_insert(w);
            assert!(is_rotation(&w.0, 1e-4));
        }
        let (a, b) = (first.unwrap(), written.unwrap());
        assert!(holds(&a, &b), "it ran away: {a:?} -> {b:?}");
    }

    #[test]
    fn sanity() {
        let ok = [IDENTITY; 3];
        assert!(sane(&ok, [1.0, 0.0, 0.0], &[[0.0, 0.0, 50.0]], [0.0; 3]).is_ok());
        let mut bad = ok;
        bad[1][4] = 1.5;
        assert!(sane(&bad, [0.0; 3], &[], [0.0; 3]).is_err());
        assert!(sane(&ok, [f32::NAN, 0.0, 0.0], &[], [0.0; 3]).is_err());
        assert!(sane(&ok, [0.0; 3], &[[0.0, 0.0, 5000.0]], [0.0; 3]).is_err());
        let (rot, t) = local_for(
            &Xf::IDENTITY,
            (IDENTITY, [0.0; 3]),
            &IDENTITY,
            Some([1e6, 0.0, 0.0]),
        );
        assert!(is_rotation(&rot, 1e-5) && length(t) <= MAX_PELVIS_OFFSET + 1e-3);
    }
}
