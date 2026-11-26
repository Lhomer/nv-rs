//! Delirium's body made from the player's own GTA IV (the PC version's files), as Delirium's
//! Playroom makes it (its `getup::homer`): the ped `m_y_pmanhat_01` from
//! `pc/models/cdimages/componentpeds.img`, opened with the archive key found in the player's
//! `GTAIV.exe`; its bones and ragdoll parts read by Delirium's data crate, the body built on the
//! character file's links (`nmdbm::asset_links`), and its parts placed in the ped's bind pose.
//! Then fitted to FNV's skeleton ([`fit_to_fnv`]). Nothing of it is kept anywhere but in memory.

use delirium::prelude::{Quat, Transform, Vec3};
use delirium::ragdoll::Build;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The ped Delirium's Playroom stands up as its man (Homer's body).
pub const PED: &str = "m_y_pmanhat_01";

/// Delirium's body from the game in `dir` (the folder of `exe`, the player's GTAIV.exe, whose
/// numbers [`crate::start`] has unlocked): the body and its parts' worlds in the ped's bind pose.
pub fn homer(dir: &Path, exe: &[u8]) -> Result<(Build, Vec<Transform>), &'static str> {
    use delirium_data::{archive_key::ArchiveKey, img, model, resource, skeleton};
    if !delirium_data::kv::open() {
        return Err("Delirium's numbers aren't unlocked");
    }
    // The PC version's files are little-endian.
    delirium_data::LE.store(true, std::sync::atomic::Ordering::Relaxed);
    let images = crate::under(dir, &["pc", "models", "cdimages"]).ok_or("no pc/models/cdimages")?;
    let mut head = vec![0u8; 0x810];
    std::fs::File::open(crate::under(&images, &["playerped.rpf"]).ok_or("no playerped.rpf")?)
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|_| "playerped.rpf can't be read")?;
    let (key, _) = ArchiveKey::find_by_trial(exe, &head).ok_or("no archive key in GTAIV.exe")?;
    let path = crate::under(&images, &["componentpeds.img"]).ok_or("no componentpeds.img")?;
    let mut f = std::fs::File::open(path).map_err(|_| "componentpeds.img can't be read")?;
    let mut table = vec![];
    (&mut f)
        .take(delirium_data::kv!(1 int) << delirium_data::kv!(128 int))
        .read_to_end(&mut table)
        .map_err(|_| "componentpeds.img can't be read")?;
    let entries = img::read_table(&table, Some(&key)).map_err(|_| "componentpeds.img's table")?;
    let name = format!("{PED}.wft");
    let e = entries
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(&name))
        .ok_or("componentpeds.img has no m_y_pmanhat_01")?;
    let mut raw = vec![];
    f.seek(SeekFrom::Start(e.offset))
        .map_err(|_| "componentpeds.img can't be read")?;
    (&mut f)
        .take(u64::from(e.used_blocks) * delirium_data::kv!(728 int))
        .read_to_end(&mut raw)
        .map_err(|_| "componentpeds.img can't be read")?;
    let frag = resource::parse(&raw).map_err(|_| "m_y_pmanhat_01 isn't one Delirium reads")?;
    let root = model::fragment_root(&frag).map_err(|_| "m_y_pmanhat_01 has no model")?;
    let bones = skeleton::read(&frag, root).map_err(|_| "m_y_pmanhat_01 has no skeleton")?;
    let parts = delirium_data::ragdoll::read(&frag).map_err(|_| "m_y_pmanhat_01 has no ragdoll")?;
    let parents: Vec<Option<usize>> = bones.iter().map(|b| b.parent).collect();
    let bind: Vec<(Vec3, Quat)> = bones
        .iter()
        .map(|b| {
            (
                Vec3::from(b.translation),
                Quat::from_array(b.rotation).normalize(),
            )
        })
        .collect();
    let links = delirium::nmdbm::asset_links(delirium::nmrs::HOMER);
    let rig = delirium::ragdoll::Rig::linked(
        &parts,
        &parents,
        &bind,
        &bones,
        delirium::nmrs::HOMER,
        links,
    )
    .ok_or("m_y_pmanhat_01 has no ragdoll")?;
    let frames = rig.place(&bind, Transform::IDENTITY);
    if rig.build.parts.len() != 21 || frames.len() != 21 {
        return Err("m_y_pmanhat_01's ragdoll isn't Delirium's 21 parts");
    }
    Ok((rig.build, frames))
}

/// Each part's joint (where it hangs from its parent; the pelvis's own place) for FNV's
/// skeleton in its bind pose, `rest` its bones' worlds in metres: the part's bone's place, and for
/// Spine1, which has no bone of FNV's, the point along FNV's spine Delirium's own body puts it at
/// (its SDK's `nm_rig_joints`: Homer's arc).
pub fn fnv_joints(names: &[&str], rest: &[Transform]) -> Option<[Vec3; 21]> {
    let bones = crate::rig::part_bones(names);
    let mut j = [Vec3::ZERO; 21];
    for p in 0..21 {
        if let Some(b) = bones[p] {
            j[p] = rest[b].translation;
        }
    }
    // Homer's arc from the pelvis to the head (its SDK's): Spine1 at 0.1628 of it.
    const ARC: [f32; 7] = [0.0, 0.0924, 0.1628, 0.2397, 0.3562, 0.5995, 0.6957];
    let chain = [0usize, 7, 9, 10, 11, 12];
    let pts: Vec<Vec3> = chain
        .iter()
        .map(|&p| bones[p].map(|b| rest[b].translation))
        .collect::<Option<_>>()?;
    let at = [0.0, ARC[1], ARC[3], ARC[4], ARC[5], ARC[6]];
    let s = ARC[2];
    let i = at.windows(2).position(|w| s >= w[0] && s <= w[1])?;
    let t = (s - at[i]) / (at[i + 1] - at[i]);
    j[crate::rig::NO_BONE] = pts[i].lerp(pts[i + 1], t);
    Some(j)
}

/// Delirium's body fitted to FNV's skeleton (its parts' joints at FNV's bones': [`fnv_joints`]).
pub fn fit_to_fnv(
    build: &Build,
    frames: &[Transform],
    joints: &[Vec3; 21],
) -> (Build, Vec<Transform>) {
    build.fit(frames, joints)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without the numbers unlocked (no GTA IV here) nothing is read: a clear refusal.
    #[test]
    fn no_body_without_the_unlock() {
        match homer(Path::new("."), b"MZ") {
            Err(e) => assert_eq!(e, "Delirium's numbers aren't unlocked"),
            Ok(_) => panic!("a body without GTA IV"),
        }
    }

    /// FNV's joints: every part's at its bone, Spine1's between Spine0's and Spine2's on the
    /// spine, in order up it.
    #[test]
    fn fnvs_joints_lie_up_its_body() {
        use ragdoll_core::np_skeleton::Skeleton;
        let sk = Skeleton::fnv();
        let names: Vec<&str> = sk.nodes.iter().map(|n| n.name.as_str()).collect();
        let rest = crate::rig::fnv_bind(&sk);
        let j = fnv_joints(&names, &rest).unwrap();
        let chain = [0usize, 7, 8, 9, 10, 11, 12];
        for w in chain.windows(2) {
            assert!(j[w[1]].z > j[w[0]].z, "{w:?} {:?} {:?}", j[w[0]], j[w[1]]);
        }
        let (a, b, c) = (j[7], j[8], j[9]);
        let along = (b - a).dot(c - a) / (c - a).length_squared();
        assert!((0.0..1.0).contains(&along), "{along}");
        assert!((b - a).cross(c - a).length() / (c - a).length() < 0.01);
        // About a person's height in metres: the head 1.5 to 1.9 m over the feet.
        let tall = j[12].z - j[3].z.min(j[6].z);
        assert!((1.3..1.9).contains(&tall), "{tall}");
    }
}
