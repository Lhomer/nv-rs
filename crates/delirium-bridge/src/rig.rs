//! FNV's human skeleton on Delirium's body: which of Delirium's 21 parts each `Bip01` bone
//! follows (`delirium_data::nm_asset::PARTS`: 0 the pelvis, 1-3 and 4-6 the legs, 7-10 the
//! spine, 11 the neck, 12 the head, 13-16 and 17-20 the arms).
//!
//! The rules are Delirium's own for another game's skeleton (its SDK's `nm_rig_part_for_name` and
//! `nm_rig_joints`), written out for FNV's names: FNV has three spine bones, so its top one
//! drives Delirium's chest (Spine3, part 10, which the neck and the clavicles hang from), its
//! middle one Spine2 (9), and Spine1 (8) has no bone (Delirium places that joint along its own
//! body's spine); both of FNV's neck bones follow the neck. Every other bone (the toes, fingers,
//! twist bones, pauldrons, the weapon, the face's) rides along with its parent, as the SDK has it.
//! Humans only: creatures' skeletons are later.

/// Delirium's parts' parents (its SDK's `PART_PARENT`): the body's tree.
pub const PART_PARENT: [Option<usize>; 21] = [
    None,
    Some(0),
    Some(1),
    Some(2),
    Some(0),
    Some(4),
    Some(5),
    Some(0),
    Some(7),
    Some(8),
    Some(9),
    Some(10),
    Some(11),
    Some(10),
    Some(13),
    Some(14),
    Some(15),
    Some(10),
    Some(17),
    Some(18),
    Some(19),
];

/// The part no FNV bone follows: Delirium's Spine1 (FNV has three spine bones to its four).
pub const NO_BONE: usize = 8;

/// The part FNV's bone `name` follows (none: it rides along with its parent).
pub fn fnv_part(name: &str) -> Option<usize> {
    let part = match name.strip_prefix("Bip01 ")? {
        "Pelvis" => 0,
        "L Thigh" => 1,
        "L Calf" => 2,
        "L Foot" => 3,
        "R Thigh" => 4,
        "R Calf" => 5,
        "R Foot" => 6,
        "Spine" => 7,
        "Spine1" => 9,
        "Spine2" => 10,
        "Neck" | "Neck1" => 11,
        "Head" => 12,
        "L Clavicle" => 13,
        "L UpperArm" => 14,
        "L Forearm" => 15,
        "L Hand" => 16,
        "R Clavicle" => 17,
        "R UpperArm" => 18,
        "R Forearm" => 19,
        "R Hand" => 20,
        _ => return None,
    };
    Some(part)
}

/// Each part's bone in `names` (the first that follows it, as Delirium's rig takes it): where the
/// part's pose is read from and written to.
pub fn part_bones(names: &[&str]) -> [Option<usize>; 21] {
    let mut out = [None; 21];
    for (b, n) in names.iter().enumerate() {
        if let Some(p) = fnv_part(n) {
            out[p] = out[p].or(Some(b));
        }
    }
    out
}

use delirium::prelude::{Quat, Transform, Vec3};

/// FNV's skeleton held to Delirium's parts (its SDK's `NmRig`, for FNV's bones): each bone that
/// follows a part takes the part's world rotation times its own offset from the part in the bind
/// pose; every other bone keeps its bind pose under its parent; the pelvis's bone its place from
/// part 0's. Only rotations are given to the bones (and the pelvis a place), so nothing stretches
/// and each bone keeps its own roll.
#[derive(Clone, Debug)]
pub struct Rig {
    parents: Vec<Option<usize>>,
    part: Vec<Option<usize>>,
    rest_local: Vec<Quat>,
    off: Vec<Quat>,
    hips: usize,
    hips_off: Vec3,
    /// Each part's frame in its bone's (its SDK's `take`): the part read from the bone's world.
    /// Spine1's, which has no bone, in Spine0's bone's.
    take: Vec<(usize, Transform)>,
}

/// FNV's human skeleton's bones' worlds in its bind pose, in Delirium's metres (`np_body::UNITS`
/// game units a metre) and transforms.
pub fn fnv_bind(sk: &ragdoll_core::np_skeleton::Skeleton) -> Vec<Transform> {
    sk.bind_worlds()
        .iter()
        .map(|x| {
            let [w, i, j, k] = ragdoll_core::active::qfrom_m(&ragdoll_core::pose::ortho(&x.r));
            Transform::from_translation(Vec3::from_array(x.t) / ragdoll_core::np_body::UNITS)
                .with_rotation(Quat::from_xyzw(i, j, k, w).normalize())
        })
        .collect()
}

impl Rig {
    /// The rig of bones `names` under `parents` (each parent before its child), whose worlds in
    /// the bind pose are `rest`, on Delirium's parts at `parts` (their worlds in that same pose).
    pub fn new(
        names: &[&str],
        parents: &[Option<usize>],
        rest: &[Transform],
        parts: &[Transform],
    ) -> Result<Rig, &'static str> {
        let n = names.len();
        if parents.len() != n || rest.len() != n || parts.len() != 21 {
            return Err("a bone list, its parents and bind worlds, and 21 parts");
        }
        if (0..n).any(|b| parents[b].is_some_and(|p| p >= b)) {
            return Err("each bone's parent must come before it");
        }
        let part: Vec<Option<usize>> = names.iter().map(|n| fnv_part(n)).collect();
        let rest_local = (0..n)
            .map(|b| match parents[b] {
                None => rest[b].rotation,
                Some(p) => rest[p].rotation.inverse() * rest[b].rotation,
            })
            .collect();
        let off = (0..n)
            .map(|b| match part[b] {
                None => Quat::IDENTITY,
                Some(k) => parts[k].rotation.inverse() * rest[b].rotation,
            })
            .collect();
        let hips = (0..n)
            .find(|&b| part[b] == Some(0))
            .ok_or("no pelvis bone")?;
        let hips_off =
            parts[0].rotation.inverse() * (rest[hips].translation - parts[0].translation);
        let bones = part_bones(names);
        let take = (0..21)
            .map(|k| {
                let b = bones[k]
                    .or(PART_PARENT[k].and_then(|q| bones[q]))
                    .ok_or("a part with no bone and no parent's")?;
                let inv = rest[b].rotation.inverse();
                Ok((
                    b,
                    Transform::from_translation(inv * (parts[k].translation - rest[b].translation))
                        .with_rotation(inv * parts[k].rotation),
                ))
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        Ok(Rig {
            parents: parents.to_vec(),
            part,
            rest_local,
            off,
            hips,
            hips_off,
            take,
        })
    }

    /// Delirium's parts' worlds for FNV's bones at `worlds` (each bone's world): each part where
    /// it lies in its bone, as in the bind pose (an actor taken mid-animation starts there).
    pub fn parts_at(&self, worlds: &[Transform]) -> Vec<Transform> {
        self.take
            .iter()
            .map(|&(b, t)| {
                let w = worlds[b];
                Transform::from_translation(w.translation + w.rotation * t.translation)
                    .with_rotation((w.rotation * t.rotation).normalize())
            })
            .collect()
    }

    /// The bones' rotations under their parents for Delirium's parts at `parts` (their worlds),
    /// and the pelvis bone's world place.
    pub fn pose(&self, parts: &[Transform]) -> (Vec<Quat>, Vec3) {
        let n = self.parents.len();
        let (mut world, mut local) = (vec![Quat::IDENTITY; n], vec![Quat::IDENTITY; n]);
        for b in 0..n {
            let parent = self.parents[b].map(|p| world[p]);
            world[b] = match self.part[b] {
                None => parent.unwrap_or(Quat::IDENTITY) * self.rest_local[b],
                Some(k) => (parts[k].rotation * self.off[b]).normalize(),
            };
            local[b] = parent.map_or(world[b], |p| p.inverse() * world[b]);
        }
        (
            local,
            parts[0].translation + parts[0].rotation * self.hips_off,
        )
    }

    /// The pelvis's bone.
    pub fn hips(&self) -> usize {
        self.hips
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ragdoll_core::np_skeleton::Skeleton;

    /// FNV's skeleton: every part but Spine1 has its bone, once (the neck's two follow the neck),
    /// and no bone Delirium's SDK leaves to ride along is given a part.
    #[test]
    fn every_part_but_spine1_has_one_of_fnvs_bones() {
        let sk = Skeleton::fnv();
        let names: Vec<&str> = sk.nodes.iter().map(|n| n.name.as_str()).collect();
        let bones = part_bones(&names);
        for (p, b) in bones.iter().enumerate() {
            assert_eq!(
                b.is_none(),
                p == NO_BONE,
                "{} {b:?}",
                delirium_data::nm_asset::PARTS[p]
            );
        }
        let mapped: Vec<(&str, usize)> = names
            .iter()
            .filter_map(|n| fnv_part(n).map(|p| (*n, p)))
            .collect();
        assert_eq!(mapped.len(), 21, "{mapped:?}");
        let twice: Vec<usize> = (0..21)
            .filter(|p| mapped.iter().filter(|m| m.1 == *p).count() > 1)
            .collect();
        assert_eq!(twice, [11]);
        for n in [
            "Bip01",
            "Bip01 NonAccum",
            "Bip01 L Toe0",
            "Bip01 LUpArmTwistBone",
            "Bip01 R ForeTwist",
            "Bip01 R ForeTwistDriver",
            "Bip01 L Finger12",
            "Bip01 R Thumb1",
            "Bip01 LPauldron",
            "Weapon",
            "HeadAnims",
            "Camera3rd",
        ] {
            assert!(names.contains(&n), "{n}");
            assert_eq!(fnv_part(n), None, "{n}");
        }
    }

    /// The parts' bones in FNV's bind pose stand as Delirium's body does: each part's bone lies
    /// below its part's parent's in FNV's tree (or, for the clavicles under FNV's neck, the
    /// chest's part is an ancestor of it there); the head over the chest over the pelvis over
    /// the knees over the feet; the left parts all on one side of the body's middle and the right
    /// all on the other (none swapped); and the arms and legs in their order away from the body.
    #[test]
    fn fnvs_bind_pose_stands_as_delirium_s_body() {
        let sk = Skeleton::fnv();
        let names: Vec<&str> = sk.nodes.iter().map(|n| n.name.as_str()).collect();
        let bones = part_bones(&names);
        let w = sk.bind_worlds();
        let at = |p: usize| w[bones[p].unwrap()].t;
        let ancestors = |b: usize| {
            let mut out = Vec::new();
            let mut a = sk.nodes[b].parent;
            while let Some(x) = a {
                out.push(x);
                a = sk.nodes[x].parent;
            }
            out
        };
        for p in (0..21).filter(|&p| p != NO_BONE) {
            // The nearest part up the body's tree that has a bone.
            let mut q = PART_PARENT[p];
            while q == Some(NO_BONE) {
                q = PART_PARENT[NO_BONE];
            }
            let Some(q) = q else { continue };
            let up = ancestors(bones[p].unwrap());
            let found = up.iter().find_map(|&a| fnv_part(names[a]));
            // FNV hangs the spine from the root's NonAccum beside the pelvis, and the
            // clavicles from the neck: the part found then is the body's sibling or child.
            let ok = found == Some(q)
                || (p == 7 && found.is_none())
                || ((p == 13 || p == 17) && found == Some(11));
            assert!(
                ok,
                "{}: up {found:?}, the body's {q}",
                names[bones[p].unwrap()]
            );
        }
        let z = |p| at(p)[2];
        assert!(z(12) > z(10) && z(10) > z(9) && z(9) > z(7) && z(7) > z(2) && z(2) > z(3));
        assert!(z(0) > z(2) && z(0) > z(5) && z(5) > z(6));
        // Sideways: FNV's skeleton faces +y, so its left is -x.
        let mid = at(0)[0];
        for p in [1, 2, 3, 13, 14, 15, 16] {
            assert!(at(p)[0] < mid, "{} on the right", names[bones[p].unwrap()]);
        }
        for p in [4, 5, 6, 17, 18, 19, 20] {
            assert!(at(p)[0] > mid, "{} on the left", names[bones[p].unwrap()]);
        }
        let out = |p: usize| (at(p)[0] - mid).abs();
        assert!(out(13) < out(14) && out(14) < out(15) && out(15) < out(16));
        assert!(out(17) < out(18) && out(18) < out(19) && out(19) < out(20));
    }

    /// The angle between two rotations (rad), exact near zero (`angle_between`'s arccosine is
    /// not: 0.07 degrees of rounding at nothing).
    fn ang(a: Quat, b: Quat) -> f32 {
        let d = a.inverse() * b;
        2.0 * d.xyz().length().atan2(d.w.abs())
    }

    /// FNV's skeleton's bind worlds as Delirium's transforms.
    fn bind() -> (Vec<String>, Vec<Option<usize>>, Vec<Transform>) {
        let sk = Skeleton::fnv();
        let names = sk.nodes.iter().map(|n| n.name.clone()).collect();
        let parents = sk.nodes.iter().map(|n| n.parent).collect();
        (names, parents, fnv_bind(&sk))
    }

    /// Parts in the bind pose turned off each bone by a made-up offset (as Delirium's own frames
    /// are): Spine1's part, with no bone, between the spine's.
    fn parts_at(names: &[&str], rest: &[Transform]) -> Vec<Transform> {
        let bones = part_bones(names);
        (0..21)
            .map(|p| {
                let b = bones[p].or(bones[7]).unwrap();
                let tilt =
                    Quat::from_euler(delirium::prelude::EulerRot::XYZ, 0.1 * p as f32, -0.2, 0.3);
                Transform::from_translation(rest[b].translation + Vec3::new(0.01, -0.02, 0.03))
                    .with_rotation(rest[b].rotation * tilt)
            })
            .collect()
    }

    /// The parts in the bind pose give FNV's bind pose back: every bone's rotation under its
    /// parent, and the pelvis where it was.
    #[test]
    fn the_bind_pose_comes_back() {
        let (names, parents, rest) = bind();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let parts = parts_at(&names, &rest);
        let rig = Rig::new(&names, &parents, &rest, &parts).unwrap();
        let (local, hips) = rig.pose(&parts);
        for b in 0..names.len() {
            let want = match parents[b] {
                None => rest[b].rotation,
                Some(p) => rest[p].rotation.inverse() * rest[b].rotation,
            };
            assert!(
                ang(local[b], want) < 1e-4,
                "{} {}",
                names[b],
                ang(local[b], want)
            );
        }
        assert!(hips.distance(rest[rig.hips()].translation) < 1e-3);
    }

    /// A forearm's part turned: the forearm's bone turns with it in the world and its hand,
    /// held by the hand's part, keeps its world; the fingers ride on the hand; nothing else moves.
    #[test]
    fn a_turned_part_turns_its_bone_only() {
        let (names, parents, rest) = bind();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut parts = parts_at(&names, &rest);
        let rig = Rig::new(&names, &parents, &rest, &parts).unwrap();
        let turn = Quat::from_axis_angle(Vec3::Z, 1.0);
        parts[15].rotation = turn * parts[15].rotation;
        let (local, _) = rig.pose(&parts);
        let mut world = vec![Quat::IDENTITY; names.len()];
        for b in 0..names.len() {
            world[b] = parents[b].map_or(local[b], |p| world[p] * local[b]);
        }
        let i = |n: &str| names.iter().position(|x| *x == n).unwrap();
        let fa = i("Bip01 L Forearm");
        assert!(ang(world[fa], turn * rest[fa].rotation) < 1e-4);
        for n in [
            "Bip01 L Hand",
            "Bip01 L UpperArm",
            "Bip01 R Forearm",
            "Bip01 Head",
        ] {
            assert!(ang(world[i(n)], rest[i(n)].rotation) < 1e-4, "{n}");
        }
        let f = i("Bip01 L Finger12");
        assert!(ang(world[f], rest[f].rotation) < 1e-4);
        // The forearm's twist bone rides on the forearm.
        let tw = i("Bip01 L ForeTwist");
        assert!(ang(world[tw], turn * rest[tw].rotation) < 1e-4);
    }

    /// The parts read from the bones and the bones posed from those parts: the same pose, in any
    /// pose (a made-up one: every bone turned under its parent), each bone within 0.006 degrees;
    /// and in the bind pose the parts come back where they were.
    #[test]
    fn parts_from_bones_and_back() {
        let (names, parents, rest) = bind();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let parts = parts_at(&names, &rest);
        let rig = Rig::new(&names, &parents, &rest, &parts).unwrap();
        let back = rig.parts_at(&rest);
        for (k, (a, b)) in back.iter().zip(&parts).enumerate() {
            if k != NO_BONE {
                assert!(a.translation.distance(b.translation) < 1e-4, "{k}");
                assert!(ang(a.rotation, b.rotation) < 1e-4, "{k}");
            }
        }
        // A pose: each bone turned a little under its parent, the worlds built down the tree.
        let n = names.len();
        let mut worlds = rest.clone();
        for b in 0..n {
            let local = match parents[b] {
                None => rest[b],
                Some(p) => {
                    let l = Transform::from_translation(
                        rest[p].rotation.inverse() * (rest[b].translation - rest[p].translation),
                    )
                    .with_rotation(rest[p].rotation.inverse() * rest[b].rotation);
                    let turn = Quat::from_euler(
                        delirium::prelude::EulerRot::XYZ,
                        0.05 * (b % 7) as f32,
                        -0.04 * (b % 5) as f32,
                        0.03 * (b % 3) as f32,
                    );
                    let l = l.with_rotation(l.rotation * turn);
                    let w = worlds[p];
                    Transform::from_translation(w.translation + w.rotation * l.translation)
                        .with_rotation(w.rotation * l.rotation)
                }
            };
            worlds[b] = local;
        }
        let (local, hips) = rig.pose(&rig.parts_at(&worlds));
        let mut w = vec![Quat::IDENTITY; n];
        let bones = part_bones(&names);
        for b in 0..n {
            w[b] = parents[b].map_or(local[b], |p| w[p] * local[b]);
            // Each part's own bone comes back as posed (the others keep their bind pose under
            // their parents or, Neck1, follow the neck's part with Neck, as Delirium's rig has it).
            if bones.contains(&Some(b)) {
                assert!(ang(w[b], worlds[b].rotation) < 1e-4, "{}", names[b]);
            }
        }
        let pelvis = bones[0].unwrap();
        assert!(hips.distance(worlds[pelvis].translation) < 1e-4);
    }

    /// A bone list out of order, or without a pelvis, is refused.
    #[test]
    fn a_bad_list_is_refused() {
        let t = [Transform::IDENTITY; 2];
        let parts = vec![Transform::IDENTITY; 21];
        assert!(Rig::new(
            &["Bip01 Pelvis", "Bip01 Spine"],
            &[Some(1), None],
            &t,
            &parts
        )
        .is_err());
        assert!(Rig::new(&["Bip01", "Bip01 Spine"], &[None, Some(0)], &t, &parts).is_err());
        assert!(Rig::new(&["Bip01 Pelvis"], &[None], &t[..1], &parts[..20]).is_err());
    }
}
