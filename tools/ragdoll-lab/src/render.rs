//! The view: an orbit camera, the scene projected into a flat list the page draws on a 2D canvas
//! (no WebGL, so it runs anywhere), and picking (a ray from the camera through a pixel onto the
//! capsules and the ground).
//!
//! The draw list is records of [`STRIDE`] floats, far ones first: `[kind, depth, x1, y1, x2, y2,
//! x3, y3, x4, y4, shade, colour]`. Kind 0: a quad or triangle of the ground (the fourth point
//! repeats the third for a triangle). Kind 1: a capsule from (x1, y1) to (x2, y2) with radii x3
//! and y3 (pixels). Kind 2: a line from (x1, y1) to (x2, y2), x3 pixels wide. Colours: 0 the
//! ground, 1 its other checker, 2 a bone, 3 the struck bone, 4 a ragdoll bone, 5 the push's
//! arrow, 6 a planted foot's mark, 7 the weapon in an armed body's hand.

use crate::mannequin::{self as mq, COUNT};
use crate::sim::Sim;
use ragdoll_core::flinch::*;
#[allow(unused_imports)]
use ragdoll_core::fm::Fm;

pub const STRIDE: usize = 12;

/// The orbit camera: around `target` (units), `yaw` and `pitch` (radians), `distance` (units),
/// the viewport (pixels).
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub target: V3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            target: [0.0, 0.0, 60.0],
            yaw: 0.45,
            pitch: 0.22,
            distance: 330.0,
            width: 800.0,
            height: 600.0,
        }
    }
}

const NEAR: f32 = 5.0;
const FOV: f32 = 0.9;

impl Camera {
    pub fn eye(&self) -> V3 {
        let (sy, cy) = self.yaw.dsin_cos();
        let (sp, cp) = self.pitch.dsin_cos();
        add(self.target, scale([sy * cp, cy * cp, sp], self.distance))
    }

    /// The camera's forward, right and up.
    fn basis(&self) -> (V3, V3, V3) {
        let f = unit(sub(self.target, self.eye())).unwrap_or([0.0, -1.0, 0.0]);
        let r = unit(cross(f, [0.0, 0.0, 1.0])).unwrap_or([1.0, 0.0, 0.0]);
        let u = cross(r, f);
        (f, r, u)
    }

    fn focal(&self) -> f32 {
        self.height / 2.0 / (FOV / 2.0).dtan()
    }

    /// A point on the screen (pixels) and its depth; `None` behind the camera.
    pub fn project(&self, p: V3) -> Option<(f32, f32, f32)> {
        let (f, r, u) = self.basis();
        let d = sub(p, self.eye());
        let z = dot(d, f);
        if z < NEAR {
            return None;
        }
        let k = self.focal() / z;
        Some((
            self.width / 2.0 + dot(d, r) * k,
            self.height / 2.0 - dot(d, u) * k,
            z,
        ))
    }

    /// The ray through a pixel: from the eye, unit direction.
    pub fn ray(&self, sx: f32, sy: f32) -> (V3, V3) {
        let (f, r, u) = self.basis();
        let k = 1.0 / self.focal();
        let d = add(
            f,
            add(
                scale(r, (sx - self.width / 2.0) * k),
                scale(u, -(sy - self.height / 2.0) * k),
            ),
        );
        (self.eye(), unit(d).unwrap_or(f))
    }
}

/// A bone's capsule now: its two end points (units) and radius.
pub fn capsule(sim: &Sim, bone: usize) -> (V3, V3, f32) {
    let (rot, pos) = sim.posed[bone];
    let (a, b, r) = mq::CAPSULE[bone];
    let local = |p: V3| scale(sub(p, mq::JOINT[bone]), sim.scale);
    (
        add(pos, apply_m(&rot, local(a))),
        add(pos, apply_m(&rot, local(b))),
        r * sim.scale,
    )
}

/// Where the ray from `from` along `dir` (unit) first meets a capsule (`a` to `b`, radius `r`):
/// its distance along the ray.
pub fn ray_capsule(from: V3, dir: V3, a: V3, b: V3, r: f32) -> Option<f32> {
    // Spheres along the segment, each intersected (cheap and robust for 20 capsules).
    let ab = sub(b, a);
    let mut best: Option<f32> = None;
    let n = 8;
    for i in 0..=n {
        let c = add(a, scale(ab, i as f32 / n as f32));
        let oc = sub(from, c);
        let bq = dot(oc, dir);
        let cq = dot(oc, oc) - r * r;
        let disc = bq * bq - cq;
        if disc >= 0.0 {
            let t = -bq - disc.sqrt();
            if t > 0.0 && best.is_none_or(|x| t < x) {
                best = Some(t);
            }
        }
    }
    best
}

/// What a ray from `from` along `dir` hits on the body first: the bone and the point.
pub fn pick_body(sim: &Sim, from: V3, dir: V3) -> Option<(usize, V3)> {
    let mut best: Option<(f32, usize)> = None;
    for b in 0..COUNT {
        if !sim.has_bone(b) {
            continue;
        }
        let (a, c, r) = capsule(sim, b);
        if let Some(t) = ray_capsule(from, dir, a, c, r) {
            if best.is_none_or(|x| t < x.0) {
                best = Some((t, b));
            }
        }
    }
    best.map(|(t, b)| (b, add(from, scale(dir, t))))
}

/// Where a ray hits the ground (the fixed colliders).
pub fn pick_ground(sim: &Sim, from: V3, dir: V3) -> Option<V3> {
    use crate::convert::{from_m, to_m, vec};
    use rapier3d::prelude::*;
    let ray = Ray::new(to_m(from), vec(dir));
    let (_, t) = sim.world.cast_ray(
        &ray,
        3000.0 / UNITS_PER_METRE,
        true,
        QueryFilter::only_fixed(),
    )?;
    Some(from_m(ray.point_at(t)))
}

/// The scene as the draw list (module documentation). `marked`: the last struck bone.
pub fn draw(sim: &Sim, cam: &Camera, marked: Option<usize>, arrow: Option<(V3, V3)>) -> Vec<f32> {
    let light = unit([0.4, 0.7, 1.0]).unwrap_or([0.0, 0.0, 1.0]);
    let mut ground: Vec<[f32; STRIDE]> = Vec::new();
    for (tri, odd) in &sim.ground.triangles {
        let (Some(p), Some(q), Some(r)) = (
            cam.project(tri[0]),
            cam.project(tri[1]),
            cam.project(tri[2]),
        ) else {
            continue;
        };
        let n = unit(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]))).unwrap_or([0.0, 0.0, 1.0]);
        // Faces turned away are not drawn.
        let centre = scale(add(add(tri[0], tri[1]), tri[2]), 1.0 / 3.0);
        if dot(n, sub(cam.eye(), centre)) < 0.0 {
            continue;
        }
        let shade = 0.55 + 0.45 * dot(n, light).abs();
        let depth = (p.2 + q.2 + r.2) / 3.0;
        ground.push([
            0.0,
            depth,
            p.0,
            p.1,
            q.0,
            q.1,
            r.0,
            r.1,
            r.0,
            r.1,
            shade,
            if *odd { 1.0 } else { 0.0 },
        ]);
    }
    ground.sort_by(|a, b| b[1].partial_cmp(&a[1]).unwrap_or(std::cmp::Ordering::Equal));
    let mut body: Vec<[f32; STRIDE]> = Vec::new();
    let focal = cam.height / 2.0 / (FOV / 2.0).dtan();
    let ragdoll = sim.in_ragdoll();
    for b in 0..COUNT {
        if !sim.has_bone(b) {
            continue;
        }
        let (a, c, r) = capsule(sim, b);
        let (Some(p), Some(q)) = (cam.project(a), cam.project(c)) else {
            continue;
        };
        let axis = unit(sub(c, a)).unwrap_or([0.0, 0.0, 1.0]);
        let to_eye = unit(sub(cam.eye(), scale(add(a, c), 0.5))).unwrap_or([0.0, 0.0, 1.0]);
        let side = unit(cross(axis, to_eye)).unwrap_or([1.0, 0.0, 0.0]);
        let shade = 0.6 + 0.4 * dot(side, light).abs();
        let colour = if Some(b) == marked {
            3.0
        } else if ragdoll {
            4.0
        } else {
            2.0
        };
        body.push([
            1.0,
            (p.2 + q.2) / 2.0,
            p.0,
            p.1,
            q.0,
            q.1,
            r * focal / p.2,
            r * focal / q.2,
            0.0,
            0.0,
            shade,
            colour,
        ]);
    }
    // An armed body's weapon: along the right forearm, out of the hand.
    if sim.armed && sim.has_bone(mq::R_HAND) {
        let hand = sim.posed[mq::R_HAND].1;
        let dir = unit(sub(hand, sim.posed[mq::R_FOREARM].1)).unwrap_or([0.0, 1.0, 0.0]);
        let a = sub(hand, scale(dir, 6.0 * sim.scale));
        let c = add(hand, scale(dir, 38.0 * sim.scale));
        if let (Some(p), Some(q)) = (cam.project(a), cam.project(c)) {
            let r = 2.2 * sim.scale;
            body.push([
                1.0,
                (p.2 + q.2) / 2.0 - 1.0,
                p.0,
                p.1,
                q.0,
                q.1,
                r * focal / p.2,
                r * focal / q.2,
                0.0,
                0.0,
                1.0,
                7.0,
            ]);
        }
    }
    // The planted feet's marks while stepping.
    if let Some(s) = &sim.rig.balance {
        for f in s.planted {
            let g = [f[0], f[1], f[2] - 5.0];
            if let (Some(p), Some(q)) = (cam.project(g), cam.project(add(g, [0.0, 9.0, 0.0]))) {
                body.push([
                    2.0,
                    p.2 + 50.0,
                    p.0,
                    p.1,
                    q.0,
                    q.1,
                    4.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    6.0,
                ]);
            }
        }
    }
    body.sort_by(|a, b| b[1].partial_cmp(&a[1]).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<f32> = Vec::with_capacity((ground.len() + body.len() + 1) * STRIDE);
    for r in ground.iter().chain(body.iter()) {
        out.extend_from_slice(r);
    }
    if let Some((at, dir)) = arrow {
        let from = sub(at, scale(dir, 60.0));
        if let (Some(p), Some(q)) = (cam.project(from), cam.project(at)) {
            out.extend_from_slice(&[2.0, 0.0, p.0, p.1, q.0, q.1, 3.0, 0.0, 0.0, 0.0, 1.0, 5.0]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_meet_capsules_and_the_camera_projects() {
        let t = ray_capsule(
            [0.0, 100.0, 50.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 40.0],
            [0.0, 0.0, 60.0],
            10.0,
        )
        .unwrap();
        assert!((t - 90.0).abs() < 1.0, "{t}");
        assert!(ray_capsule(
            [0.0, 100.0, 50.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 40.0],
            [0.0, 0.0, 60.0],
            10.0
        )
        .is_none());
        let cam = Camera::default();
        let (x, y, _) = cam.project(cam.target).unwrap();
        assert!((x - cam.width / 2.0).abs() < 1e-3 && (y - cam.height / 2.0).abs() < 1e-3);
        let (from, dir) = cam.ray(cam.width / 2.0, cam.height / 2.0);
        let to = unit(sub(cam.target, from)).unwrap();
        assert!(dot(dir, to) > 0.9999);
    }
}
