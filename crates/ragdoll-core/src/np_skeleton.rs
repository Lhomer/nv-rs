//! New Physics' skeleton: FNV's own (`meshes\characters\_male\skeleton.nif`, the male and female
//! humans'), from its derived data (`data/fnv_skeleton.txt`): the nodes (names, parents, local
//! transforms), the 18 ragdoll bodies (the node each hangs from, its mass and capsule) and the 17
//! constraints between them with their limits (Patrick, 2026-11-17: supplied from the skeleton
//! file rather than read from the game's `bhk` constraints). New Physics' body (`np_body`, M1)
//! limits its joints with these; the host's skeleton fixture lays the same nodes out in the
//! engine's memory.
//!
//! The constraints' vectors and pivots are in each body's own frame. A ragdoll body is a plain
//! `bhkRigidBody`, so its frame is its node's world transform (checked within 0.04 degrees); a
//! pivot is in Havok units (times [`HAVOK_UNITS`] for game units). Body A is the child
//! (`entities[0]`), body B the parent (`entities[1]`).
//!
//! What a joint is measured as ([`Joint::measure`]), Havok's own definitions:
//! - a ragdoll joint (`bhkRagdollConstraint`, or one wrapped in a `bhkMalleableConstraint`): the
//!   cone, the angle between the twist axes (A's and B's); the plane, the angle of A's twist axis
//!   out of the plane whose normal is B's plane axis; the twist, the turn of A's plane axis from B's
//!   about the twist axis;
//! - a limited hinge (wrapped in a malleable constraint): the angle, the turn from B's zero
//!   reference `perp2AxleIn1` to A's about A's axle (with that sign every hinge's bind pose lies
//!   inside its limits: the knee 69.9, increasing as it straightens; the elbow -4.5, decreasing as it
//!   bends). The arms' hinges' axles are 15 degrees apart in the bind pose.
//!
//! The data's own bind pose: the legs', spine's, neck's and head's pivots meet within 0.2 game
//! units, the arms' do not (the shoulder's 1.1 apart, the elbow's 2.2, the wrist's 2.0: the elbow's
//! pivot lies 19.6 units along the upper arm, its node 17.4); the neck lies 20 degrees out of its
//! 6.8 degree cone and its plane, so Havok's constraint pulls it in.

use crate::flinch::{apply_m, cross, dot, length, mul, sub, unit, M3, V3};
#[allow(unused_imports)]
use crate::fm::Fm;
use crate::pose::{self, Xf};

/// FNV's skeleton's derived data.
pub const FNV_SKELETON: &str = include_str!("../data/fnv_skeleton.txt");

/// Game units to a Havok unit.
pub const HAVOK_UNITS: f32 = 7.0;

/// A node of the skeleton.
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    /// Its local transform (the rows as the NIF stores them taken as the transpose of the matrix
    /// the game applies).
    pub local: Xf,
}

/// A ragdoll body.
#[derive(Clone, Debug)]
pub struct Body {
    /// The node it hangs from.
    pub node: String,
    /// kg.
    pub mass: f32,
    /// Its capsule's radius and end points, in its own frame (Havok units).
    pub radius: f32,
    pub ends: [V3; 2],
}

/// What a constraint limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Limit {
    /// A ragdoll joint: twist, plane and motor axes in A's frame and in B's; the cone's largest
    /// angle, the plane's and the twist's least and most (radians).
    Ragdoll {
        twist: [V3; 2],
        plane: [V3; 2],
        motor: [V3; 2],
        cone: f32,
        planes: (f32, f32),
        twists: (f32, f32),
    },
    /// A limited hinge: the axle and the two references (`perp2AxleIn1`, `perp2AxleIn2`) in A's
    /// frame and in B's; the angle's least and most (radians).
    Hinge {
        axle: [V3; 2],
        perp1: [V3; 2],
        perp2: [V3; 2],
        angles: (f32, f32),
    },
}

/// A constraint between two bodies.
#[derive(Clone, Debug)]
pub struct Joint {
    /// Body A's node (the child) and body B's (the parent).
    pub child: String,
    pub parent: String,
    /// The malleable constraint's strength wrapping it (none for a plain ragdoll constraint).
    pub strength: Option<f32>,
    /// The pivots, in A's frame and in B's (Havok units).
    pub pivot: [V3; 2],
    pub limit: Limit,
}

/// A joint as it is now (radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Measure {
    Ragdoll { cone: f32, plane: f32, twist: f32 },
    Hinge { angle: f32 },
}

impl Joint {
    /// The joint measured from body A's world rotation `a` and body B's `b` (each its node's).
    pub fn measure(&self, a: &M3, b: &M3) -> Measure {
        match self.limit {
            Limit::Ragdoll { twist, plane, .. } => {
                let (ta, tb) = (apply_m(a, twist[0]), apply_m(b, twist[1]));
                let (pa, pb) = (apply_m(a, plane[0]), apply_m(b, plane[1]));
                let cone = dot(ta, tb).clamp(-1.0, 1.0).dacos();
                let plane_angle = dot(ta, pb).clamp(-1.0, 1.0).dasin();
                // The twist: A's plane axis against B's, both off the twist axis, about it.
                let axis = unit(crate::flinch::add(ta, tb)).unwrap_or(ta);
                let flat = |v: V3| unit(sub(v, crate::flinch::scale(axis, dot(v, axis))));
                let twist_angle = match (flat(pa), flat(pb)) {
                    (Some(x), Some(y)) => dot(cross(y, x), axis).datan2(dot(y, x)),
                    _ => 0.0,
                };
                Measure::Ragdoll {
                    cone,
                    plane: plane_angle,
                    twist: twist_angle,
                }
            }
            Limit::Hinge { axle, perp1, .. } => {
                let ax = apply_m(a, axle[0]);
                let (ra, rb) = (apply_m(a, perp1[0]), apply_m(b, perp1[1]));
                Measure::Hinge {
                    angle: dot(cross(rb, ra), ax).datan2(dot(rb, ra)),
                }
            }
        }
    }

    /// How far `m` lies outside this joint's limits (radians; 0 inside).
    pub fn outside(&self, m: Measure) -> f32 {
        let past = |x: f32, (lo, hi): (f32, f32)| (lo - x).max(x - hi).max(0.0);
        match (self.limit, m) {
            (
                Limit::Ragdoll {
                    cone,
                    planes,
                    twists,
                    ..
                },
                Measure::Ragdoll {
                    cone: c,
                    plane,
                    twist,
                },
            ) => (c - cone)
                .max(0.0)
                .max(past(plane, planes))
                .max(past(twist, twists)),
            (Limit::Hinge { angles, .. }, Measure::Hinge { angle }) => past(angle, angles),
            _ => f32::INFINITY,
        }
    }
}

/// The skeleton: its nodes (parents before children), bodies and joints.
#[derive(Clone, Debug, Default)]
pub struct Skeleton {
    pub nodes: Vec<Node>,
    pub bodies: Vec<Body>,
    pub joints: Vec<Joint>,
}

impl Skeleton {
    /// FNV's skeleton ([`FNV_SKELETON`]).
    pub fn fnv() -> Skeleton {
        parse(FNV_SKELETON).expect("FNV's skeleton's data parses")
    }

    /// The node named `name`.
    pub fn node(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    /// Every node's world in its bind pose (the root at the origin).
    pub fn bind_worlds(&self) -> Vec<Xf> {
        let mut out: Vec<Xf> = Vec::with_capacity(self.nodes.len());
        for n in &self.nodes {
            let w = match n.parent {
                Some(p) => out[p].then(&n.local),
                None => n.local,
            };
            out.push(w);
        }
        out
    }

    /// The joint whose child body hangs from `child`.
    pub fn joint(&self, child: &str) -> Option<&Joint> {
        self.joints.iter().find(|j| j.child == child)
    }
}

/// A transform's rotation as the game applies it from the NIF's stored rows.
fn from_rows(rows: &[f32]) -> M3 {
    pose::ortho(&std::array::from_fn(|k| rows[(k % 3) * 3 + k / 3]))
}

fn numbers(s: &str) -> Result<Vec<f32>, String> {
    s.split([',', ' ', ';'])
        .filter(|x| !x.is_empty())
        .map(|x| x.parse::<f32>().map_err(|e| format!("{x:?}: {e}")))
        .collect()
}

fn v3(s: &str) -> Result<V3, String> {
    let n = numbers(s)?;
    if n.len() != 3 {
        return Err(format!("{s:?} is not three numbers"));
    }
    Ok([n[0], n[1], n[2]])
}

fn pair(s: &str) -> Result<(f32, f32), String> {
    let n = numbers(s)?;
    if n.len() != 2 {
        return Err(format!("{s:?} is not two numbers"));
    }
    Ok((n[0], n[1]))
}

/// The derived data parsed (its three sections: nodes, rigid bodies, constraints).
pub fn parse(text: &str) -> Result<Skeleton, String> {
    #[derive(PartialEq)]
    enum Section {
        Nodes,
        Bodies,
        Constraints,
    }
    let mut section = Section::Nodes;
    let mut sk = Skeleton::default();
    for line in text.lines() {
        if line.starts_with("# RIGID BODIES") {
            section = Section::Bodies;
            continue;
        }
        if line.starts_with("# CONSTRAINTS") {
            section = Section::Constraints;
            continue;
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('|').collect();
        match section {
            Section::Nodes => {
                if f.len() < 10 || (f[4] != "NiNode" && f[4] != "BSFadeNode") {
                    continue;
                }
                let parent = if f[3].is_empty() {
                    None
                } else {
                    Some(
                        sk.node(f[3])
                            .ok_or_else(|| format!("{}'s parent {} not before it", f[2], f[3]))?,
                    )
                };
                let rows = numbers(f[6])?;
                if rows.len() != 9 {
                    return Err(format!("{}: rows {:?}", f[2], f[6]));
                }
                sk.nodes.push(Node {
                    name: f[2].to_string(),
                    parent,
                    local: Xf {
                        r: from_rows(&rows),
                        t: v3(f[5])?,
                        s: f[8].parse().map_err(|e| format!("{}: scale: {e}", f[2]))?,
                    },
                });
            }
            Section::Bodies => {
                if f.len() < 8 {
                    return Err(format!("a rigid body line {line:?}"));
                }
                let cap = f[7];
                let r = cap
                    .split_whitespace()
                    .find_map(|w| w.strip_prefix('r'))
                    .ok_or_else(|| format!("{}: no capsule radius", f[1]))?;
                let end = |key: &str| -> Result<V3, String> {
                    let i = cap.find(key).ok_or_else(|| format!("{}: no {key}", f[1]))?;
                    let rest = &cap[i + key.len()..];
                    let close = rest.find(')').ok_or_else(|| format!("{}: {key}", f[1]))?;
                    v3(&rest[..close])
                };
                sk.bodies.push(Body {
                    node: f[1].to_string(),
                    mass: f[3].parse().map_err(|e| format!("{}: mass: {e}", f[1]))?,
                    radius: r.parse().map_err(|e| format!("{}: radius: {e}", f[1]))?,
                    ends: [end("p1(")?, end("p2(")?],
                });
            }
            Section::Constraints => {
                if f.len() != 8 {
                    return Err(format!("a constraint line {line:?}"));
                }
                let mut axes: Vec<(&str, V3)> = Vec::new();
                for part in f[6].split(';') {
                    let (name, v) = part
                        .split_once(' ')
                        .ok_or_else(|| format!("{}: axis {part:?}", f[0]))?;
                    axes.push((name, v3(v)?));
                }
                let axis = |name: &str| -> Result<V3, String> {
                    axes.iter()
                        .find(|a| a.0 == name)
                        .map(|a| a.1)
                        .ok_or_else(|| format!("{}: no {name}", f[0]))
                };
                let mut limits: Vec<(&str, &str)> = Vec::new();
                for part in f[7].split(';') {
                    limits.push(
                        part.split_once(' ')
                            .ok_or_else(|| format!("{}: limit {part:?}", f[0]))?,
                    );
                }
                let limit_of = |name: &str| -> Result<&str, String> {
                    limits
                        .iter()
                        .find(|l| l.0 == name)
                        .map(|l| l.1)
                        .ok_or_else(|| format!("{}: no {name} limit", f[0]))
                };
                let limit = match f[2] {
                    "MRD" | "RD" => Limit::Ragdoll {
                        twist: [axis("tA")?, axis("tB")?],
                        plane: [axis("plA")?, axis("plB")?],
                        motor: [axis("mA")?, axis("mB")?],
                        cone: limit_of("cone")?
                            .parse()
                            .map_err(|e| format!("{}: cone: {e}", f[0]))?,
                        planes: pair(limit_of("plane")?)?,
                        twists: pair(limit_of("twist")?)?,
                    },
                    "MLH" => Limit::Hinge {
                        axle: [axis("axA")?, axis("axB")?],
                        perp1: [axis("p1A")?, axis("p1B")?],
                        perp2: [axis("p2A")?, axis("p2B")?],
                        angles: pair(limit_of("angle")?)?,
                    },
                    k => return Err(format!("{}: kind {k}", f[0])),
                };
                let strength = match f[3] {
                    "-" => None,
                    s => Some(s.parse().map_err(|e| format!("{}: strength: {e}", f[0]))?),
                };
                if (f[2] == "RD") != strength.is_none() {
                    return Err(format!("{}: a malleable without a strength", f[0]));
                }
                sk.joints.push(Joint {
                    child: f[0].to_string(),
                    parent: f[1].to_string(),
                    strength,
                    pivot: [v3(f[4])?, v3(f[5])?],
                    limit,
                });
            }
        }
    }
    Ok(sk)
}

/// The world rotation `m` taken onto `v`'s frame (`m^T v`).
pub fn to_local(m: &M3, v: V3) -> V3 {
    apply_m(&crate::flinch::transpose(m), v)
}

/// The world position of a body's local point `p` (Havok units) on a body at `world` (its node's).
pub fn world_point(world: &Xf, p: V3) -> V3 {
    crate::flinch::add(
        world.t,
        apply_m(&world.r, crate::flinch::scale(p, HAVOK_UNITS)),
    )
}

/// The turn from `b`'s rotation to `a`'s (`a b^T`), its angle (radians).
pub fn angle_between(a: &M3, b: &M3) -> f32 {
    length(crate::flinch::rotation_vector(&mul(
        a,
        &crate::flinch::transpose(b),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deg(x: f32) -> f32 {
        x.to_degrees()
    }

    /// The data as the skeleton has it: 66 nodes (65 `NiNode`s and the root), 18 bodies of 81 kg,
    /// 17 joints (14 malleable at 0.9, the spine's 3 plain), each between a body and its parent
    /// body, each child body in exactly one.
    #[test]
    fn the_skeleton_parses() {
        let sk = Skeleton::fnv();
        assert_eq!(sk.nodes.len(), 66);
        assert_eq!(sk.bodies.len(), 18);
        let mass: f32 = sk.bodies.iter().map(|b| b.mass).sum();
        assert!((mass - 81.0).abs() < 1e-3, "{mass}");
        assert_eq!(sk.joints.len(), 17);
        assert_eq!(
            sk.joints.iter().filter(|j| j.strength == Some(0.9)).count(),
            14
        );
        let plain: Vec<&str> = sk
            .joints
            .iter()
            .filter(|j| j.strength.is_none())
            .map(|j| j.child.as_str())
            .collect();
        assert_eq!(plain, ["Bip01 Spine", "Bip01 Spine1", "Bip01 Spine2"]);
        for j in &sk.joints {
            assert!(sk.bodies.iter().any(|b| b.node == j.child), "{}", j.child);
            assert!(sk.bodies.iter().any(|b| b.node == j.parent), "{}", j.parent);
        }
        // Every body but the root one is some joint's child, once.
        for b in &sk.bodies {
            let n = sk.joints.iter().filter(|j| j.child == b.node).count();
            assert_eq!(n, usize::from(b.node != "Bip01 NonAccum"), "{}", b.node);
        }
    }

    /// The joints' pivots in the bind pose (body A's and body B's, each from its node's world):
    /// the legs', spine's, neck's and head's meet within 0.25 game units (the thigh's knee pivot
    /// 4.563 Havok units, 31.9 game units, its length); the arms' lie apart as the data has them
    /// (the shoulder's 1.06, the elbow's 2.23, the wrist's 1.96).
    #[test]
    fn the_pivots_in_the_bind_pose() {
        let sk = Skeleton::fnv();
        let w = sk.bind_worlds();
        for j in &sk.joints {
            let a = w[sk.node(&j.child).unwrap()];
            let b = w[sk.node(&j.parent).unwrap()];
            let gap = length(sub(
                world_point(&a, j.pivot[0]),
                world_point(&b, j.pivot[1]),
            ));
            let want = match j
                .child
                .trim_start_matches("Bip01 ")
                .trim_start_matches(['L', 'R', ' '])
            {
                "UpperArm" => 1.06,
                "Forearm" => 2.23,
                "Hand" => 1.96,
                _ => 0.0,
            };
            assert!(
                (gap - want).abs() < 0.25,
                "{} > {}: pivots {gap:.2} apart, want {want}",
                j.child,
                j.parent
            );
        }
    }

    /// The bind pose against the limits (Patrick's diagnostics, measured from the same file): the
    /// hinges' angles (the knees 69.9, the ankles 2.5, the elbows -4.5, the wrists -2.5 and 1.5)
    /// inside their limits; the ragdoll joints' cones (the thighs 3.2, the spine 3.9, 3.7 and 10.0,
    /// the neck 20.0, the head 0, the upper arms 3.6), the Spine2 at its plane limit and the neck
    /// outside its 6.8 degree cone.
    #[test]
    fn the_bind_pose_against_the_limits() {
        let sk = Skeleton::fnv();
        let w = sk.bind_worlds();
        let measure = |child: &str| {
            let j = sk.joint(child).unwrap();
            let a = w[sk.node(&j.child).unwrap()].r;
            let b = w[sk.node(&j.parent).unwrap()].r;
            (j.measure(&a, &b), j.outside(j.measure(&a, &b)))
        };
        let hinge = |child: &str, want: f32| {
            let (m, out) = measure(child);
            let Measure::Hinge { angle } = m else {
                panic!("{child}: not a hinge")
            };
            assert!(
                (deg(angle) - want).abs() < 0.5,
                "{child}: {:.1} degrees, want {want}",
                deg(angle)
            );
            assert_eq!(out, 0.0, "{child}: outside its limits");
        };
        for side in ["L", "R"] {
            hinge(&format!("Bip01 {side} Calf"), 69.9);
            hinge(&format!("Bip01 {side} Foot"), 2.5);
            hinge(&format!("Bip01 {side} Forearm"), -4.5);
        }
        hinge("Bip01 L Hand", -2.5);
        hinge("Bip01 R Hand", 1.5);
        let cone = |child: &str, want: f32| {
            let (m, out) = measure(child);
            let Measure::Ragdoll { cone, plane, .. } = m else {
                panic!("{child}: not a ragdoll joint")
            };
            assert!(
                (deg(cone) - want).abs() < 0.5,
                "{child}: cone {:.1} degrees, want {want}",
                deg(cone)
            );
            (deg(plane), out.to_degrees())
        };
        for side in ["L", "R"] {
            assert_eq!(cone(&format!("Bip01 {side} Thigh"), 3.2).1, 0.0);
            assert_eq!(cone(&format!("Bip01 {side} UpperArm"), 3.6).1, 0.0);
        }
        assert_eq!(cone("Bip01 Spine", 3.9).1, 0.0);
        assert_eq!(cone("Bip01 Spine1", 3.7).1, 0.0);
        let (plane, out) = cone("Bip01 Spine2", 10.0);
        assert!(
            (plane.abs() - 10.0).abs() < 0.5 && out < 0.5,
            "Spine2 plane {plane:.1}, out {out:.2}"
        );
        assert_eq!(cone("Bip01 Head", 0.0).1, 0.0);
        // The neck: 20 degrees out, past its 6.8 degree cone and below its plane's -3.3.
        let (plane, out) = cone("Bip01 Neck1", 20.0);
        assert!((plane + 20.0).abs() < 0.5, "the neck's plane {plane:.1}");
        assert!(
            (out - (20.0 - 3.3)).abs() < 0.6,
            "the neck {out:.1} degrees past its limits"
        );
    }

    /// Left and right mirror each other (the thighs' plane limits apart: the left's least -50.4,
    /// the right's -30, as stored).
    #[test]
    fn left_and_right_mirror() {
        let sk = Skeleton::fnv();
        for (l, r) in [
            ("Bip01 L Calf", "Bip01 R Calf"),
            ("Bip01 L Foot", "Bip01 R Foot"),
            ("Bip01 L UpperArm", "Bip01 R UpperArm"),
            ("Bip01 L Forearm", "Bip01 R Forearm"),
            ("Bip01 L Hand", "Bip01 R Hand"),
        ] {
            let (a, b) = (sk.joint(l).unwrap(), sk.joint(r).unwrap());
            match (a.limit, b.limit) {
                (Limit::Hinge { angles: x, .. }, Limit::Hinge { angles: y, .. }) => {
                    assert_eq!(x, y, "{l}")
                }
                (
                    Limit::Ragdoll {
                        cone: c,
                        planes: p,
                        twists: t,
                        ..
                    },
                    Limit::Ragdoll {
                        cone: d,
                        planes: q,
                        twists: u,
                        ..
                    },
                ) => {
                    assert_eq!((c, p, t), (d, q, u), "{l}")
                }
                _ => panic!("{l} and {r} differ in kind"),
            }
        }
        let planes = |c: &str| match sk.joint(c).unwrap().limit {
            Limit::Ragdoll { planes, .. } => planes,
            _ => panic!(),
        };
        let in_degrees = |(lo, hi): (f32, f32)| {
            (
                (lo.to_degrees() * 10.0).round() / 10.0,
                (hi.to_degrees() * 10.0).round() / 10.0,
            )
        };
        assert_eq!(in_degrees(planes("Bip01 L Thigh")), (-50.4, 30.0));
        assert_eq!(in_degrees(planes("Bip01 R Thigh")), (-30.0, 30.0));
    }

    /// A hinge measured past its limit says by how much; a knee straightened 20 degrees past the
    /// bind pose is 2.6 past its 87.3.
    #[test]
    fn a_joint_past_its_limit_says_how_far() {
        let sk = Skeleton::fnv();
        let w = sk.bind_worlds();
        let j = sk.joint("Bip01 L Calf").unwrap();
        let (a, b) = (
            w[sk.node(&j.child).unwrap()].r,
            w[sk.node(&j.parent).unwrap()].r,
        );
        let Limit::Hinge { axle, .. } = j.limit else {
            panic!()
        };
        let ax = apply_m(&b, axle[1]);
        let turn = |d: f32| crate::flinch::rotation(crate::flinch::scale(ax, d.to_radians()));
        let straighter = mul(&turn(20.0), &a);
        let Measure::Hinge { angle } = j.measure(&straighter, &b) else {
            panic!()
        };
        assert!((deg(angle) - 89.9).abs() < 0.5, "{}", deg(angle));
        assert!((deg(j.outside(j.measure(&straighter, &b))) - 2.6).abs() < 0.5);
        let _ = to_local(&a, ax);
        let _ = angle_between(&a, &b);
    }
}
