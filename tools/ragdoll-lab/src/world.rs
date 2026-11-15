//! The lab's ground: flat, a slope, uneven ground, stairs down, a wall and a ledge, as Rapier's fixed
//! colliders (the ragdoll falls on them) and as triangles to draw. [`ground_point`] stands in for
//! the game's navmesh query (`crate::knockdown::navmesh_point` in the host): the walkable ground
//! under a point.

use crate::convert::{from_m, to_m};
use ragdoll_core::flinch::V3;
use rapier3d::prelude::*;

/// The grounds the lab has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroundKind {
    Flat,
    Slope,
    Uneven,
    Stairs,
    Wall,
    Ledge,
}

pub const GROUNDS: [(&str, GroundKind); 6] = [
    ("flat", GroundKind::Flat),
    ("slope", GroundKind::Slope),
    ("uneven", GroundKind::Uneven),
    ("stairs", GroundKind::Stairs),
    ("wall", GroundKind::Wall),
    ("ledge", GroundKind::Ledge),
];

impl GroundKind {
    pub fn named(name: &str) -> Option<GroundKind> {
        GROUNDS
            .iter()
            .find(|g| g.0.eq_ignore_ascii_case(name.trim()))
            .map(|g| g.1)
    }
    pub fn name(self) -> &'static str {
        GROUNDS.iter().find(|g| g.1 == self).map_or("flat", |g| g.0)
    }
}

/// The slope's angle (degrees): the ground rises toward +Y, so a push toward the default camera's
/// far side (-Y) goes downhill.
pub const SLOPE_DEGREES: f32 = 15.0;
/// Where the wall's face is, the ledge's edge and the stairs' first step, along -Y (units).
pub const WALL_AT: f32 = -25.0;
pub const LEDGE_AT: f32 = -20.0;
pub const LEDGE_DROP: f32 = 150.0;
pub const STAIRS_AT: f32 = -15.0;
pub const STAIR_RISE: f32 = 16.0;
pub const STAIR_RUN: f32 = 30.0;

/// A built ground: its triangles to draw (game units, within [`DRAWN`] of the start), each with
/// its checker square's parity (1 m squares, to see the body move).
pub struct Ground {
    pub kind: GroundKind,
    pub triangles: Vec<([V3; 3], bool)>,
}

/// How far from the start the ground is drawn (units); the colliders go farther.
pub const DRAWN: f32 = 420.0;
/// The checker's square (units).
pub const TILE: f32 = 70.0;

type Tris = Vec<([V3; 3], bool)>;

/// A face `origin + a u + b v` for `a` in `ua` and `b` in `vb` (units along the unit vectors `u`
/// and `v`), cut into checker squares.
fn tile(tris: &mut Tris, origin: V3, u: V3, v: V3, ua: (f32, f32), vb: (f32, f32)) {
    let at = |a: f32, b: f32| {
        [
            origin[0] + u[0] * a + v[0] * b,
            origin[1] + u[1] * a + v[1] * b,
            origin[2] + u[2] * a + v[2] * b,
        ]
    };
    let cuts = |lo: f32, hi: f32| {
        let mut c = vec![lo];
        let mut x = (lo / TILE).floor() * TILE + TILE;
        while x < hi {
            c.push(x);
            x += TILE;
        }
        c.push(hi);
        c
    };
    let (ca, cb) = (cuts(ua.0, ua.1), cuts(vb.0, vb.1));
    for i in 0..ca.len() - 1 {
        for j in 0..cb.len() - 1 {
            let (a0, a1, b0, b1) = (ca[i], ca[i + 1], cb[j], cb[j + 1]);
            if a1 - a0 < 1e-3 || b1 - b0 < 1e-3 {
                continue;
            }
            let odd = (((a0 + a1) / 2.0 / TILE).floor() as i64
                + ((b0 + b1) / 2.0 / TILE).floor() as i64)
                .rem_euclid(2)
                == 1;
            let (p, q, r, w) = (at(a0, b0), at(a1, b0), at(a1, b1), at(a0, b1));
            tris.push(([p, q, r], odd));
            tris.push(([p, r, w], odd));
        }
    }
}

/// A box as a fixed collider and its five visible faces, in game units: `min` and `max` corners.
fn add_box(world: &mut PhysicsWorld, tris: &mut Tris, min: V3, max: V3) {
    let half = [
        (max[0] - min[0]) / 2.0,
        (max[1] - min[1]) / 2.0,
        (max[2] - min[2]) / 2.0,
    ];
    let centre = [
        (max[0] + min[0]) / 2.0,
        (max[1] + min[1]) / 2.0,
        (max[2] + min[2]) / 2.0,
    ];
    let c = ColliderBuilder::cuboid(half[0] / 70.0, half[1] / 70.0, half[2] / 70.0)
        .translation(to_m(centre))
        .friction(0.9)
        .collision_groups(ground_groups())
        .build();
    world.insert_collider(c, None);
    // The faces drawn: within the drawn square.
    let x0 = min[0].max(-DRAWN);
    let x1 = max[0].min(DRAWN);
    let y0 = min[1].max(-DRAWN);
    let y1 = max[1].min(DRAWN);
    let (z0, z1) = (min[2], max[2]);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let (ex, ey, ez) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    tile(tris, [0.0, 0.0, z1], ex, ey, (x0, x1), (y0, y1));
    if min[1] > -DRAWN {
        tile(tris, [0.0, y0, 0.0], ex, ez, (x0, x1), (z0, z1));
    }
    if max[1] < DRAWN {
        tile(tris, [0.0, y1, 0.0], ex, ez, (x0, x1), (z0, z1));
    }
    if min[0] > -DRAWN {
        tile(tris, [x0, 0.0, 0.0], ey, ez, (y0, y1), (z0, z1));
    }
    if max[0] < DRAWN {
        tile(tris, [x1, 0.0, 0.0], ey, ez, (y0, y1), (z0, z1));
    }
}

/// The ground's collision groups: everything hits it.
pub fn ground_groups() -> InteractionGroups {
    InteractionGroups::new(Group::GROUP_1, Group::ALL, InteractionTestMode::And)
}

/// The uneven ground's height at (x, y) (units).
pub fn bumps(x: f32, y: f32) -> f32 {
    6.0 * (x / 40.0).sin() * (y / 55.0).cos() + 4.0 * ((x + y) / 23.0).sin() - 4.0
}

/// Builds `kind` into `world` (which holds no other fixed collider).
pub fn build(world: &mut PhysicsWorld, kind: GroundKind) -> Ground {
    let mut tris = Vec::new();
    let far = 700.0;
    match kind {
        GroundKind::Flat => add_box(world, &mut tris, [-far, -far, -40.0], [far, far, 0.0]),
        GroundKind::Slope => {
            // A plane through the origin rising toward +Y.
            let a = SLOPE_DEGREES.to_radians();
            let (s, c) = a.sin_cos();
            let size = far;
            let corners = [
                [-size, -size * c, -size * s],
                [size, -size * c, -size * s],
                [size, size * c, size * s],
                [-size, size * c, size * s],
            ];
            let verts: Vec<_> = corners.iter().map(|&p| to_m(p)).collect();
            let plane = ColliderBuilder::trimesh(verts, vec![[0, 1, 2], [0, 2, 3]])
                .expect("two triangles")
                .friction(0.9)
                .collision_groups(ground_groups())
                .build();
            world.insert_collider(plane, None);
            tile(
                &mut tris,
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, c, s],
                (-DRAWN, DRAWN),
                (-DRAWN, DRAWN),
            );
        }
        GroundKind::Uneven => {
            let n = 30usize;
            let size = 600.0f32;
            let step = 2.0 * size / n as f32;
            let mut verts = Vec::new();
            let mut pts = Vec::new();
            for j in 0..=n {
                for i in 0..=n {
                    let (x, y) = (-size + i as f32 * step, -size + j as f32 * step);
                    let p = [x, y, bumps(x, y)];
                    pts.push(p);
                    verts.push(to_m(p));
                }
            }
            let mut idx = Vec::new();
            for j in 0..n {
                for i in 0..n {
                    let a = (j * (n + 1) + i) as u32;
                    let b = a + 1;
                    let d = a + (n + 1) as u32;
                    let c = d + 1;
                    idx.push([a, b, c]);
                    idx.push([a, c, d]);
                    let odd = (i + j) % 2 == 1;
                    if pts[a as usize][0].abs() <= DRAWN && pts[a as usize][1].abs() <= DRAWN {
                        tris.push(([pts[a as usize], pts[b as usize], pts[c as usize]], odd));
                        tris.push(([pts[a as usize], pts[c as usize], pts[d as usize]], odd));
                    }
                }
            }
            let c = ColliderBuilder::trimesh(verts, idx)
                .expect("a grid")
                .friction(0.9)
                .collision_groups(ground_groups())
                .build();
            world.insert_collider(c, None);
        }
        GroundKind::Stairs => {
            add_box(world, &mut tris, [-far, STAIRS_AT, -40.0], [far, far, 0.0]);
            let mut y = STAIRS_AT;
            for i in 1..=8 {
                let top = -STAIR_RISE * i as f32;
                add_box(
                    world,
                    &mut tris,
                    [-far, y - STAIR_RUN, top - 40.0],
                    [far, y, top],
                );
                y -= STAIR_RUN;
            }
            let top = -STAIR_RISE * 9.0;
            add_box(world, &mut tris, [-far, -far, top - 40.0], [far, y, top]);
        }
        GroundKind::Wall => {
            add_box(world, &mut tris, [-far, -far, -40.0], [far, far, 0.0]);
            add_box(
                world,
                &mut tris,
                [-200.0, WALL_AT - 12.0, 0.0],
                [200.0, WALL_AT, 180.0],
            );
        }
        GroundKind::Ledge => {
            add_box(world, &mut tris, [-far, LEDGE_AT, -40.0], [far, far, 0.0]);
            add_box(
                world,
                &mut tris,
                [-far, -far, -LEDGE_DROP - 40.0],
                [far, LEDGE_AT, -LEDGE_DROP],
            );
        }
    }
    // The queries (the ground under a point, the picking) read the broad phase, which a step
    // brings up to date.
    world.step();
    Ground {
        kind,
        triangles: tris,
    }
}

/// The walkable ground under `at` (units): a ray down from well above it onto the fixed
/// colliders, the first surface no steeper than 50 degrees. `None` for none (no ground near).
pub fn ground_point(world: &PhysicsWorld, at: V3) -> Option<V3> {
    let from = [at[0], at[1], at[2] + 200.0];
    let ray = Ray::new(to_m(from), Vec3::new(0.0, 0.0, -1.0));
    let filter = QueryFilter::only_fixed();
    let (_, hit) = world.cast_ray_and_get_normal(&ray, 800.0 / 70.0, true, filter)?;
    if hit.normal.z < 0.64 {
        return None;
    }
    let p = ray.point_at(hit.time_of_impact);
    Some(from_m(p))
}

/// How far ahead of `at` along `dir` (flat) a fixed surface is (units), within `range` (units):
/// the host's `wall_ahead`, by the game's ray cast there.
pub fn wall_ahead(world: &PhysicsWorld, at: V3, dir: V3, range: f32) -> Option<f32> {
    let l = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    if !(l > 1e-6 && l.is_finite()) {
        return None;
    }
    let ray = Ray::new(to_m(at), Vec3::new(dir[0] / l, dir[1] / l, 0.0));
    let (_, hit) =
        world.cast_ray_and_get_normal(&ray, range / 70.0, true, QueryFilter::only_fixed())?;
    // A face near upright (not a ramp or a stair's tread); the host tells them apart by a second,
    // higher cast.
    (hit.normal.z.abs() < 0.45).then_some(hit.time_of_impact * 70.0)
}

/// Whether a fixed surface stands between a foot at `from` and its step's target `to`, at knee
/// height (the host's `wall_between`).
pub fn wall_between(world: &PhysicsWorld, from: V3, to: V3) -> bool {
    let d = [to[0] - from[0], to[1] - from[1], 0.0];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    len > 1e-3 && wall_ahead(world, [from[0], from[1], from[2] + 25.0], d, len + 8.0).is_some()
}
