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
/// The most a later pass's move of a written bone can be and still be taken for a disturbance
/// ([`Read::Disturbed`]): rotation entries over [`ROT_TOL`] (50: about 6 degrees) and translation
/// (game units: the game's foot IK moves the pelvis by several, and a pelvis move taken for the
/// animation's stacked its offset every frame: Patrick's 9mm clip, the body 2.5 m off in 0.7 s
/// after a 0.3 m/s flinch); farther, the animation wrote it. An animation moving a bone the way
/// its offset goes is never held off by more than this.
pub const DISTURBED_MOST: f32 = 50.0;
pub const DISTURBED_MOVE: f32 = 12.0;
/// The most an animated local moves under a reacting body in one frame ([`capped`]): rotation
/// (radians), translation (game units). An animation's pop is eased in over frames.
pub const MOST_BASE_TURN: f32 = 0.35;
pub const MOST_BASE_MOVE: f32 = 15.0;

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

/// How a bone's local was read ([`read_base`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// It still holds what was written: the remembered animated pose stays.
    Held,
    /// It holds what was written moved by a later pass (beyond [`ROT_TOL`] / [`POS_TOL`], but
    /// nearer the write than the remembered pose): the remembered pose stays, the move is not taken
    /// for the animation's.
    Disturbed,
    /// The animation wrote it: what it holds now is the animated pose.
    Animated,
}

/// How far apart two locals are, in tolerances: the larger of the rotations' largest entry
/// difference over [`ROT_TOL`] and the translations' distance over [`POS_TOL`].
pub fn apart(a: &(M3, V3), b: &(M3, V3)) -> f32 {
    let r =
        a.0.iter()
            .zip(b.0.iter())
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
    (r / ROT_TOL).max(length(sub(a.1, b.1)) / POS_TOL)
}

/// The animated local under the layer and how it was read ([`Read`]): the remembered pose while
/// the bone still holds what was written (nothing wrote it again, or the game's pass wrote it back
/// a hair off), and also while it holds what was written moved by a later pass (nearer the write
/// than the remembered pose: taking that for the animation's pose put the offset on top
/// of itself every frame, and the body drifted away until the animation wrote the bone again:
/// Patrick, 2026-11-16); else what it holds now (the animation wrote it). Its rotation is made
/// orthonormal.
///
/// The rotation is read whole; the translation axis by axis, so a pass that sets one axis outright
/// (the game's foot IK puts the pelvis's height, leaving the rest as written) is taken on that
/// axis alone: read whole, the write's other axes went into the base with it, and the pelvis's
/// sideways offset stacked every frame (Patrick's 9mm clip: 2.5 m in 0.7 s).
pub fn read_base(
    now: (M3, V3),
    pose: Option<(M3, V3)>,
    written: Option<(M3, V3)>,
) -> ((M3, V3), Read) {
    let (Some(p), Some(w)) = (pose, written) else {
        return ((ortho(&now.0), now.1), Read::Animated);
    };
    if holds(&now, &w) {
        return (p, Read::Held);
    }
    // The whole local moved a little off the write, nearer it than the remembered pose.
    let whole = near_write(&now, &w) && apart(&now, &w) < apart(&now, &p);
    let mut how = Read::Held;
    let mut seen = |read: Read| {
        how = match (how, read) {
            (Read::Animated, _) | (_, Read::Animated) => Read::Animated,
            (Read::Disturbed, _) | (_, Read::Disturbed) => Read::Disturbed,
            _ => Read::Held,
        }
    };
    let rot_off = |a: &M3, b: &M3| {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max)
    };
    let r = if rot_off(&now.0, &w.0) <= ROT_TOL {
        seen(Read::Held);
        p.0
    } else if whole
        || (rot_off(&now.0, &w.0) / ROT_TOL <= DISTURBED_MOST
            && rot_off(&now.0, &w.0) < rot_off(&now.0, &p.0))
    {
        seen(Read::Disturbed);
        p.0
    } else {
        seen(Read::Animated);
        ortho(&now.0)
    };
    let mut t = now.1;
    for (i, ti) in t.iter_mut().enumerate() {
        let off = (now.1[i] - w.1[i]).abs();
        if off <= POS_TOL {
            seen(Read::Held);
            *ti = p.1[i];
        } else if whole || (off <= DISTURBED_MOVE && off < (now.1[i] - p.1[i]).abs()) {
            seen(Read::Disturbed);
            *ti = p.1[i];
        } else {
            seen(Read::Animated);
        }
    }
    ((r, t), how)
}

/// Whether a read is within a later pass's move of the write ([`DISTURBED_MOST`],
/// [`DISTURBED_MOVE`]).
fn near_write(now: &(M3, V3), w: &(M3, V3)) -> bool {
    let r = now
        .0
        .iter()
        .zip(w.0.iter())
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max);
    r / ROT_TOL <= DISTURBED_MOST && length(sub(now.1, w.1)) <= DISTURBED_MOVE
}

/// `next` (an animated local) from `prev` held to [`MOST_BASE_TURN`] and [`MOST_BASE_MOVE`]
/// (`k` of them: a frame's share), and whether it was held back.
pub fn capped(prev: &(M3, V3), next: &(M3, V3), k: f32) -> ((M3, V3), bool) {
    use crate::active::{m_from_q, qconj, qexp, qfrom_m, qlog, qmul, qnorm};
    let (qa, qb) = (qfrom_m(&prev.0), qfrom_m(&next.0));
    let mut d = qnorm(qmul(qb, qconj(qa)));
    if d[0] < 0.0 {
        d = d.map(|x| -x);
    }
    let turn = qlog(d);
    let angle = length(turn);
    let move_ = sub(next.1, prev.1);
    let (most_turn, most_move) = (MOST_BASE_TURN * k, MOST_BASE_MOVE * k);
    let cut_turn = angle > most_turn;
    let cut_move = length(move_) > most_move;
    if !(cut_turn || cut_move) {
        return (*next, false);
    }
    let r = if cut_turn {
        ortho(&m_from_q(qnorm(qmul(
            qexp(scale(turn, most_turn / angle)),
            qa,
        ))))
    } else {
        next.0
    };
    let t = if cut_move {
        add(prev.1, scale(move_, most_move / length(move_)))
    } else {
        next.1
    };
    ((r, t), true)
}

/// [`read_base`]'s pose alone.
pub fn base_of(now: (M3, V3), pose: Option<(M3, V3)>, written: Option<(M3, V3)>) -> (M3, V3) {
    read_base(now, pose, written).0
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

    /// A bone the animation stops writing, its write moved past the tolerance by a later pass each
    /// frame (Patrick, 2026-11-16: the body drifted 168 units in 2 s, then snapped back): the
    /// remembered pose stays, so the offset never goes on top of itself; a fresh animated pose (near
    /// the remembered one, far from the write) is taken.
    #[test]
    fn a_disturbed_write_is_no_new_animation() {
        use crate::flinch::rotation;
        let base: (M3, V3) = (rotation([0.1, 0.2, 0.0]), [10.0, 0.0, 0.0]);
        let offset = rotation([0.0, 0.0, 0.4]);
        let shift = [6.0, 0.0, 0.0];
        let parent = Xf::IDENTITY;
        let mut pose = Some(base);
        let mut written: Option<(M3, V3)> = None;
        let mut now = base;
        for frame in 0..120 {
            let (b, how) = read_base(now, pose, written);
            if frame > 0 {
                assert_eq!(how, Read::Disturbed, "frame {frame}");
                assert!(apart(&b, &base) < 1e-3, "frame {frame}: the base drifted");
            }
            pose = Some(b);
            let w = local_for(&parent, b, &offset, Some(shift));
            written = Some(w);
            // A later pass moves the write by 0.2 units and 0.004 (past both tolerances); the
            // animation writes nothing.
            let mut moved = w;
            moved.1[2] += 0.2;
            moved.0[1] += 0.004;
            now = moved;
        }
        // The animation writes again, near the remembered pose: taken.
        let fresh = (base.0, [10.5, 0.0, 0.0]);
        let (b, how) = read_base(fresh, pose, written);
        assert_eq!(how, Read::Animated);
        assert_eq!(b.1, fresh.1);
        // A write held within the tolerance: the remembered pose.
        assert_eq!(read_base(written.unwrap(), pose, written).1, Read::Held);
        // No offset (the write is the base): a moved bone is the animation's.
        let (_, how) = read_base(fresh, Some(base), Some(base));
        assert_eq!(how, Read::Animated);
    }

    /// A pass that sets one axis outright (the game's foot IK: the pelvis's height) is taken on
    /// that axis alone: the sideways offset written never goes into the base (read whole, it
    /// stacked every frame: Patrick's 9mm clip, 2.5 m in 0.7 s).
    #[test]
    fn a_pass_setting_one_axis_takes_that_axis_alone() {
        let base: (M3, V3) = (IDENTITY, [10.0, 0.0, 50.0]);
        let shift = [2.0, 1.0, 1.0];
        let mut pose = Some(base);
        let mut written: Option<(M3, V3)> = None;
        let mut now = base;
        for frame in 0..120 {
            let (b, _) = read_base(now, pose, written);
            pose = Some(b);
            assert!(
                (b.1[0] - 10.0).abs() < 1e-3 && b.1[1].abs() < 1e-3,
                "frame {frame}: the sideways offset went into the base: {:?}",
                b.1
            );
            // The height: the animation's or the IK's, never drifting.
            assert!(
                (b.1[2] - 46.0).abs() < 1e-3 || (b.1[2] - 50.0).abs() < 1e-3,
                "frame {frame}: the height drifted to {}",
                b.1[2]
            );
            let w = local_for(&Xf::IDENTITY, b, &IDENTITY, Some(shift));
            written = Some(w);
            // No animation write; the foot IK sets the height 4 under the animation's.
            now = (w.0, [w.1[0], w.1[1], 46.0]);
        }
    }

    /// An animation moving a bone the way its offset goes is taken once it is past
    /// [`DISTURBED_MOVE`] from the write: the base never stalls far behind it.
    #[test]
    fn an_animation_along_the_offset_is_taken() {
        let base: (M3, V3) = (IDENTITY, [10.0, 0.0, 0.0]);
        let shift = [2.0, 0.0, 0.0];
        let mut pose = Some(base);
        let mut written: Option<(M3, V3)> = None;
        let mut anim = base;
        for frame in 0..60 {
            let (b, _) = read_base(anim, pose, written);
            pose = Some(b);
            // Behind by at most the bound, the shift and a frame's move.
            let behind = length(sub(b.1, anim.1));
            assert!(
                behind <= DISTURBED_MOVE + 2.0 + 1.5 + 1e-3,
                "frame {frame}: the base is {behind:.1} units behind the animation"
            );
            written = Some(local_for(&Xf::IDENTITY, b, &IDENTITY, Some(shift)));
            // The animation runs the bone along the shift, 1.5 units a frame (past half the shift:
            // each read is nearer the write than the base).
            anim.1[0] += 1.5;
        }
    }

    /// A pop in the animation under a reacting body is eased in: a frame's share at most, the
    /// rest the frames after; a small move goes through as it is.
    #[test]
    fn an_animation_pop_is_capped() {
        use crate::flinch::rotation;
        let a: (M3, V3) = (IDENTITY, [0.0; 3]);
        let b: (M3, V3) = (rotation([0.0, 0.0, 1.2]), [40.0, 0.0, 0.0]);
        let (c, cut) = capped(&a, &b, 1.0);
        assert!(cut);
        assert!((length(c.1) - MOST_BASE_MOVE).abs() < 1e-3);
        let turned = crate::active::qlog(crate::active::qfrom_m(&c.0));
        assert!((length(turned) - MOST_BASE_TURN).abs() < 1e-3, "{turned:?}");
        let mut now = a;
        for _ in 0..4 {
            now = capped(&now, &b, 1.0).0;
        }
        assert!(apart(&now, &b) < 1.0, "caught up: {}", apart(&now, &b));
        let small = (rotation([0.0, 0.0, 0.05]), [1.0, 0.0, 0.0]);
        assert_eq!(capped(&a, &small, 1.0), (small, false));
    }
}
