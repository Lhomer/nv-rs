//! Havok's world near a body, for Delirium's collision (its `Collision`: triangle meshes in
//! chunks, added and dropped as the body moves). Havok keeps the world; what Delirium needs of it is
//! what New Physics already finds by casting into it around each body (`host::new_physics`: each
//! body's floor plane, the walls beside it, step edges and crates as boxes). Each becomes a small
//! mesh here: a plane a square around where the cast met it, a step's edge its top and face, a
//! crate its top and four sides. Metres, Z up, as New Physics' and Delirium's are; each
//! triangle's corners turn counter-clockwise seen from the side the surface faces.

use ragdoll_core::np_body::{Edge, Plane};

type V3 = [f32; 3];

/// Triangles: corners and, three a triangle, the corners they join.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    /// `other`'s triangles added.
    pub fn add(&mut self, other: &Mesh) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.triangles
            .extend(other.triangles.iter().map(|t| t.map(|i| i + base)));
    }

    /// A quad of corners `a b c d` (counter-clockwise from its front) as two triangles.
    fn quad(&mut self, [a, b, c, d]: [V3; 4]) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&[a, b, c, d]);
        self.triangles.push([base, base + 1, base + 2]);
        self.triangles.push([base, base + 2, base + 3]);
    }

    /// Each triangle's unit normal (its corners' turn).
    pub fn normals(&self) -> Vec<V3> {
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.positions[i as usize]);
                unit(cross(sub(b, a), sub(c, a)))
            })
            .collect()
    }
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn unit(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l > 1e-9 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Two directions across a plane of normal `n` (unit), `u × v = n`.
fn across(n: V3) -> (V3, V3) {
    let n = unit(n);
    let seed = if n[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let u = unit(cross(seed, n));
    (u, cross(n, u))
}

/// A plane New Physics found as a square `half` each way around `at` (projected onto it),
/// facing its normal.
pub fn plane(pl: &Plane, at: V3, half: f32) -> Mesh {
    let n = unit(pl.n);
    let c = sub(at, scale(n, dot(sub(at, pl.p), n)));
    let (u, v) = across(n);
    let (u, v) = (scale(u, half), scale(v, half));
    let mut m = Mesh::default();
    m.quad([
        sub(sub(c, u), v),
        sub(add(c, u), v),
        add(add(c, u), v),
        add(sub(c, u), v),
    ]);
    m
}

/// A wall New Physics found as a square `half` each way around `at` (projected onto it), facing
/// its normal, cut off at its top (`top`, metres; infinite: none known): nothing of it over the
/// wall's own top (a square reaching over a railing's or a low wall's top was a wall in the air
/// a body coming down over it was shoved out of).
pub fn wall(pl: &Plane, at: V3, half: f32, top: f32) -> Mesh {
    let n = unit(pl.n);
    let c = sub(at, scale(n, dot(sub(at, pl.p), n)));
    let (u, v) = across(n);
    // Up the face: from `half` under its centre to `half` over it, or to the top.
    let up = if v[2] > 0.05 && top.is_finite() {
        ((top - c[2]) / v[2]).min(half)
    } else {
        half
    };
    let mut m = Mesh::default();
    if up <= -half {
        return m;
    }
    let (u, below, over) = (scale(u, half), scale(v, -half), scale(v, up));
    m.quad([
        add(sub(c, u), below),
        add(add(c, u), below),
        add(add(c, u), over),
        add(sub(c, u), over),
    ]);
    m
}

/// A step's or crate's edge, down to `floor` (metres): its face under the edge (facing away from
/// its top), and for a crate (`top` over 0) its top and the other three sides; a step's top as
/// deep as `step_top` (the floors hold the rest).
pub fn edge(ed: &Edge, floor: f32, step_top: f32) -> Mesh {
    let mut m = Mesh::default();
    let dir = unit([ed.dir[0], ed.dir[1], 0.0]);
    let up = unit([ed.up[0], ed.up[1], 0.0]);
    let h = ed.p[2];
    if h <= floor {
        return m;
    }
    let depth = if ed.top > 0.0 { ed.top } else { step_top };
    let at = |along: f32, across: f32, z: f32| {
        let p = add(add(ed.p, scale(dir, along)), scale(up, across));
        [p[0], p[1], z]
    };
    let r = ed.reach;
    // The four corners of the top, the edge's two first.
    let (a, b, c, d) = (
        at(-r, 0.0, h),
        at(r, 0.0, h),
        at(r, depth, h),
        at(-r, depth, h),
    );
    let below = |p: V3| [p[0], p[1], floor];
    // The top faces up whichever way the edge runs.
    if dot(cross(sub(b, a), sub(d, a)), [0.0, 0.0, 1.0]) > 0.0 {
        m.quad([a, b, c, d]);
    } else {
        m.quad([d, c, b, a]);
    }
    let mut side = |p: V3, q: V3, out: V3| {
        // A wall from `p` to `q` (on the top's rim), facing `out`.
        let face = [below(p), below(q), q, p];
        let n = cross(sub(face[1], face[0]), sub(face[3], face[0]));
        if dot(n, out) > 0.0 {
            m.quad(face);
        } else {
            m.quad([face[3], face[2], face[1], face[0]]);
        }
    };
    side(a, b, scale(up, -1.0));
    if ed.top > 0.0 {
        side(b, c, dir);
        side(c, d, up);
        side(d, a, scale(dir, -1.0));
    }
    m
}

/// The ground as a height field: `n` by `n` heights (row by row along y, each along x) `spacing`
/// apart, its first corner at `corner` (x, y), every triangle facing up. What the game's collision
/// has under and around a body (cast down at each point: rocks, steps and slopes as they are), one
/// surface with no seams between bodies' squares.
pub fn heightfield(corner: [f32; 2], spacing: f32, n: usize, heights: &[f32]) -> Mesh {
    let mut m = Mesh::default();
    if n < 2 || heights.len() < n * n {
        return m;
    }
    for j in 0..n {
        for i in 0..n {
            m.positions.push([
                corner[0] + i as f32 * spacing,
                corner[1] + j as f32 * spacing,
                heights[j * n + i],
            ]);
        }
    }
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = (j * n + i) as u32;
            let (b, c, d) = (a + 1, a + 1 + n as u32, a + n as u32);
            m.triangles.push([a, b, c]);
            m.triangles.push([a, c, d]);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A height field faces up everywhere, two triangles a cell, its corners where the heights
    /// put them.
    #[test]
    fn a_height_field_faces_up() {
        let n = 4;
        let heights: Vec<f32> = (0..n * n).map(|k| (k % 3) as f32 * 0.1).collect();
        let m = heightfield([-1.0, 2.0], 0.5, n, &heights);
        assert_eq!(m.triangles.len(), 2 * (n - 1) * (n - 1));
        assert!(
            m.normals().iter().all(|nr| nr[2] > 0.5),
            "{:?}",
            m.normals()
        );
        assert!(near(m.positions[n + 1], [-0.5, 2.5, heights[n + 1]]));
        assert!(heightfield([0.0, 0.0], 1.0, 3, &[0.0; 4])
            .triangles
            .is_empty());
    }

    fn near(a: V3, b: V3) -> bool {
        sub(a, b).iter().all(|x| x.abs() < 1e-4)
    }

    /// A floor plane becomes a square around where it was met, on the plane, facing up; a wall
    /// plane faces out of the wall; the square is as big as asked.
    #[test]
    fn a_plane_is_a_square_facing_its_normal() {
        for n in [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], unit([0.0, -0.3, 1.0])] {
            let pl = Plane {
                p: [1.0, 2.0, 0.5],
                n,
            };
            let m = plane(&pl, [1.2, 2.1, 3.0], 0.75);
            assert_eq!((m.positions.len(), m.triangles.len()), (4, 2));
            for nn in m.normals() {
                assert!(near(nn, unit(n)), "{nn:?} {n:?}");
            }
            for p in &m.positions {
                assert!(dot(sub(*p, pl.p), unit(n)).abs() < 1e-4);
            }
            let side = sub(m.positions[1], m.positions[0]);
            assert!((dot(side, side).sqrt() - 1.5).abs() < 1e-4);
        }
    }

    /// A crate: a top facing up at the edge's height, and four sides facing out of it, down to
    /// the floor; its footprint the edge's reach both ways and its top's depth.
    #[test]
    fn a_crate_is_a_box_with_its_faces_out() {
        let ed = Edge {
            p: [0.0, 0.0, 0.6],
            dir: [1.0, 0.0, 0.0],
            reach: 0.4,
            up: [0.0, 1.0, 0.0],
            top: 0.5,
        };
        let m = edge(&ed, 0.0, 0.3);
        assert_eq!(m.triangles.len(), 10);
        let centre = [0.0, 0.25, 0.3];
        for (t, n) in m.triangles.iter().zip(m.normals()) {
            let mid = scale(
                t.iter()
                    .fold([0.0; 3], |s, &i| add(s, m.positions[i as usize])),
                1.0 / 3.0,
            );
            assert!(dot(n, sub(mid, centre)) > 0.0, "{t:?} faces in: {n:?}");
        }
        let (lo, hi) = m
            .positions
            .iter()
            .fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
                (
                    [lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])],
                    [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])],
                )
            });
        assert!(
            near(lo, [-0.4, 0.0, 0.0]) && near(hi, [0.4, 0.5, 0.6]),
            "{lo:?} {hi:?}"
        );
    }

    /// A step: its top (as deep as asked) and its face under the edge, facing down the step;
    /// nothing for an edge at or under the floor.
    #[test]
    fn a_step_is_its_top_and_face() {
        let ed = Edge {
            p: [0.0, 0.0, 0.2],
            dir: [0.0, -1.0, 0.0],
            reach: 1.0,
            up: [1.0, 0.0, 0.0],
            top: 0.0,
        };
        let m = edge(&ed, 0.0, 0.3);
        assert_eq!(m.triangles.len(), 4);
        let ns = m.normals();
        assert!(
            near(ns[0], [0.0, 0.0, 1.0]) && near(ns[2], [-1.0, 0.0, 0.0]),
            "{ns:?}"
        );
        assert!(edge(
            &Edge {
                p: [0.0, 0.0, -0.1],
                ..ed
            },
            0.0,
            0.3
        )
        .triangles
        .is_empty());
    }

    /// Meshes add up with their corners renumbered.
    #[test]
    fn meshes_add() {
        let pl = Plane {
            p: [0.0; 3],
            n: [0.0, 0.0, 1.0],
        };
        let mut m = plane(&pl, [0.0; 3], 1.0);
        m.add(&plane(&pl, [5.0, 0.0, 0.0], 1.0));
        assert_eq!(m.positions.len(), 8);
        assert_eq!(m.triangles[2], [4, 5, 6]);
        assert!(m
            .triangles
            .iter()
            .flatten()
            .all(|&i| (i as usize) < m.positions.len()));
    }
}
