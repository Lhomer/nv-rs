//! The floor of a place for the Test Range ([`crate::range_cells`]): its navmesh's flat
//! triangles laid on a grid, the clearance of each grid square from the mesh's edge, and the
//! widest, longest clear rectangle on it ([`best_rect`]), where the range's lanes run.
//!
//! **Why.** Patrick (097ad477): the range loaded into `NorthernPassageINT`, a narrow passage, and
//! the targets stood in its walls. The first ranking measured only how far a cell's references
//! spread. A navmesh is where the game lets actors walk: a rectangle of it that is flat, a step
//! in from its edge and wide enough is floor a target can stand on and the player can shoot
//! down.
//!
//! **The grid** ([`Floor`]): square cells of [`Floor::res`] game units in a frame turned by
//! [`Floor::angle`] (interiors are tried at 0 and 45 degrees). A grid square is floor when its
//! centre lies in a navmesh triangle that is flat (within [`FLAT`] of level); where two floors
//! overlap more than a metre apart (a walkway over a hall) the square is left out, as is a square
//! a steep triangle covers. The clearance of a square is its distance in squares to the nearest
//! square that is not floor (the grid's own edge counts as not floor).
//!
//! **The rectangle** ([`best_rect`]): among the largest rectangles of squares with at least the
//! wanted clearance (each row's histogram, every maximal rectangle), the one with the best
//! [`rect_score`]: at least [`MIN_WIDTH`] m wide and [`MIN_LENGTH`] m long, its width counting as
//! much as its length (to [`WIDE`] m and [`LONG`] m), less below [`GOOD_WIDTH`] m, less again for
//! a floor that is not level.
//!
//! Not 1:1 with FalloutNV.exe: the range is an addition (docs/GUNPLAY.md, slice 2).

/// Game units in a metre.
pub const UNITS_PER_METRE: f32 = 70.0;
/// The steepest triangle counted as floor: the cosine of its tilt (about 8 degrees).
pub const FLAT: f32 = 0.99;
/// Two floors closer than this (in units) at one grid square are one floor (a seam); farther
/// apart, the square is left out.
const LAYER: f32 = 1.0 * UNITS_PER_METRE;
/// The narrowest and shortest rectangle the range takes, the width it wants, and where width and
/// length stop counting, in metres.
pub const MIN_WIDTH: f32 = 8.0;
pub const MIN_LENGTH: f32 = 25.0;
pub const GOOD_WIDTH: f32 = 12.0;
pub const WIDE: f32 = 30.0;
pub const LONG: f32 = 110.0;

/// A navmesh's floor on a grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Floor {
    /// The frame's turn: the grid's `u` axis is `(cos, sin)` of it in the world's x, y.
    pub angle: f32,
    /// The frame position of the grid's first square's corner, in units.
    pub u0: f32,
    pub v0: f32,
    /// Units per square.
    pub res: f32,
    pub w: usize,
    pub h: usize,
    /// Each square's floor height (`NAN`: not floor).
    pub z: Vec<f32>,
    /// Each square's clearance in squares (0: not floor).
    pub clear: Vec<u16>,
}

/// A rectangle of squares, `i` along `u`, `j` along `v`, the ends exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub i0: usize,
    pub j0: usize,
    pub i1: usize,
    pub j1: usize,
}

/// Where the range's lanes run, in the world: the player's place at one end (the floor's
/// height), the heading down the rectangle, its length and width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lane {
    pub start: [f32; 3],
    pub heading: f32,
    pub length_m: f32,
    pub width_m: f32,
    /// The highest floor less the lowest inside the rectangle, in metres.
    pub spread_m: f32,
}

impl Floor {
    /// An empty grid over the frame box `u` in `[u0, u1)`, `v` in `[v0, v1)`.
    pub fn empty(angle: f32, res: f32, u0: f32, v0: f32, u1: f32, v1: f32) -> Floor {
        let w = (((u1 - u0) / res).ceil().max(1.0)) as usize;
        let h = (((v1 - v0) / res).ceil().max(1.0)) as usize;
        Floor {
            angle,
            u0,
            v0,
            res,
            w,
            h,
            z: vec![f32::NAN; w * h],
            clear: vec![0; w * h],
        }
    }

    /// A grid fitted to `triangles` (world points), turned by `angle`, `res` units a square, with
    /// the triangles laid on it ([`Floor::lay`]).
    pub fn of(triangles: &[[[f32; 3]; 3]], angle: f32, res: f32) -> Option<Floor> {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for t in triangles {
            for p in t {
                let (u, v) = to_frame(angle, p[0], p[1]);
                lo = [lo[0].min(u), lo[1].min(v)];
                hi = [hi[0].max(u), hi[1].max(v)];
            }
        }
        if lo[0] > hi[0] {
            return None;
        }
        let mut floor = Floor::empty(angle, res, lo[0], lo[1], hi[0] + res, hi[1] + res);
        let mut layered = vec![false; floor.z.len()];
        for t in triangles {
            floor.lay(t, &mut layered);
        }
        floor.finish(&layered);
        Some(floor)
    }

    /// Lays one triangle: its flat part on the squares whose centres it covers. `layered` marks
    /// the squares two floors (or a steep triangle) cover, left out by [`Floor::finish`].
    pub fn lay(&mut self, t: &[[f32; 3]; 3], layered: &mut [bool]) {
        let [a, b, c] = *t;
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length.is_nan() || length <= 1e-3 {
            return;
        }
        let flat = n[2].abs() >= FLAT * length;
        let p: Vec<(f32, f32, f32)> = t
            .iter()
            .map(|q| {
                let (u, v) = to_frame(self.angle, q[0], q[1]);
                (u, v, q[2])
            })
            .collect();
        let (u_lo, u_hi) = (
            p[0].0.min(p[1].0).min(p[2].0),
            p[0].0.max(p[1].0).max(p[2].0),
        );
        let (v_lo, v_hi) = (
            p[0].1.min(p[1].1).min(p[2].1),
            p[0].1.max(p[1].1).max(p[2].1),
        );
        let cell = |x: f32, origin: f32, n: usize| -> Option<usize> {
            let k = ((x - origin) / self.res).floor();
            (k >= 0.0).then_some((k as usize).min(n.saturating_sub(1)))
        };
        let (Some(i_lo), Some(i_hi)) = (cell(u_lo, self.u0, self.w), cell(u_hi, self.u0, self.w))
        else {
            return;
        };
        let (Some(j_lo), Some(j_hi)) = (cell(v_lo, self.v0, self.h), cell(v_hi, self.v0, self.h))
        else {
            return;
        };
        let d = (p[1].1 - p[2].1) * (p[0].0 - p[2].0) + (p[2].0 - p[1].0) * (p[0].1 - p[2].1);
        if d.abs() < 1e-6 {
            return;
        }
        for j in j_lo..=j_hi {
            let v = self.v0 + (j as f32 + 0.5) * self.res;
            for i in i_lo..=i_hi {
                let u = self.u0 + (i as f32 + 0.5) * self.res;
                let l1 = ((p[1].1 - p[2].1) * (u - p[2].0) + (p[2].0 - p[1].0) * (v - p[2].1)) / d;
                let l2 = ((p[2].1 - p[0].1) * (u - p[2].0) + (p[0].0 - p[2].0) * (v - p[2].1)) / d;
                let l3 = 1.0 - l1 - l2;
                let e = -1e-4;
                if l1 < e || l2 < e || l3 < e {
                    continue;
                }
                let k = j * self.w + i;
                if !flat {
                    layered[k] = true;
                    continue;
                }
                let z = l1 * p[0].2 + l2 * p[1].2 + l3 * p[2].2;
                let old = self.z[k];
                if old.is_nan() {
                    self.z[k] = z;
                } else if (old - z).abs() > LAYER {
                    layered[k] = true;
                }
            }
        }
    }

    /// Leaves out the squares `layered` marks and works out the clearances.
    pub fn finish(&mut self, layered: &[bool]) {
        for (z, &l) in self.z.iter_mut().zip(layered) {
            if l {
                *z = f32::NAN;
            }
        }
        self.clearance();
    }

    /// Leaves out the squares within `radius` units of the world point (x, y) (a square around
    /// it: an actor's surroundings in an exterior). Call [`Floor::clearance`] after.
    pub fn exclude(&mut self, x: f32, y: f32, radius: f32) {
        let (u, v) = to_frame(self.angle, x, y);
        let lo_i = ((u - radius - self.u0) / self.res).floor().max(0.0) as usize;
        let lo_j = ((v - radius - self.v0) / self.res).floor().max(0.0) as usize;
        let hi_i = ((u + radius - self.u0) / self.res).ceil();
        let hi_j = ((v + radius - self.v0) / self.res).ceil();
        if hi_i < 0.0 || hi_j < 0.0 {
            return;
        }
        let hi_i = (hi_i as usize).min(self.w);
        let hi_j = (hi_j as usize).min(self.h);
        for j in lo_j..hi_j {
            for i in lo_i..hi_i {
                self.z[j * self.w + i] = f32::NAN;
            }
        }
    }

    /// Each square's distance in squares to the nearest square that is not floor (the 8-way
    /// chamfer distance, the grid's edge counting as not floor).
    pub fn clearance(&mut self) {
        let (w, h) = (self.w, self.h);
        let mut d: Vec<u16> = self
            .z
            .iter()
            .map(|z| if z.is_nan() { 0 } else { u16::MAX })
            .collect();
        let at = |d: &[u16], i: isize, j: isize| -> u16 {
            if i < 0 || j < 0 || i >= w as isize || j >= h as isize {
                0
            } else {
                d[j as usize * w + i as usize]
            }
        };
        for j in 0..h as isize {
            for i in 0..w as isize {
                let k = j as usize * w + i as usize;
                if d[k] == 0 {
                    continue;
                }
                let m = at(&d, i - 1, j)
                    .min(at(&d, i, j - 1))
                    .min(at(&d, i - 1, j - 1))
                    .min(at(&d, i + 1, j - 1));
                d[k] = d[k].min(m.saturating_add(1));
            }
        }
        for j in (0..h as isize).rev() {
            for i in (0..w as isize).rev() {
                let k = j as usize * w + i as usize;
                if d[k] == 0 {
                    continue;
                }
                let m = at(&d, i + 1, j)
                    .min(at(&d, i, j + 1))
                    .min(at(&d, i + 1, j + 1))
                    .min(at(&d, i - 1, j + 1));
                d[k] = d[k].min(m.saturating_add(1));
            }
        }
        self.clear = d;
    }

    /// The floor at the world point (x, y): its height and clearance in metres, `None` off the
    /// floor.
    pub fn at(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let (u, v) = to_frame(self.angle, x, y);
        let i = ((u - self.u0) / self.res).floor();
        let j = ((v - self.v0) / self.res).floor();
        if i < 0.0 || j < 0.0 || i as usize >= self.w || j as usize >= self.h {
            return None;
        }
        let k = j as usize * self.w + i as usize;
        let z = self.z[k];
        (!z.is_nan()).then(|| (z, f32::from(self.clear[k]) * self.res / UNITS_PER_METRE))
    }

    /// The part of the grid within `margin` squares of `r` (what a place keeps for checking its
    /// targets' spots).
    pub fn crop(&self, r: Rect, margin: usize) -> Floor {
        let i0 = r.i0.saturating_sub(margin);
        let j0 = r.j0.saturating_sub(margin);
        let i1 = (r.i1 + margin).min(self.w);
        let j1 = (r.j1 + margin).min(self.h);
        let (w, h) = (i1 - i0, j1 - j0);
        let mut z = Vec::with_capacity(w * h);
        let mut clear = Vec::with_capacity(w * h);
        for j in j0..j1 {
            z.extend_from_slice(&self.z[j * self.w + i0..j * self.w + i1]);
            clear.extend_from_slice(&self.clear[j * self.w + i0..j * self.w + i1]);
        }
        Floor {
            angle: self.angle,
            u0: self.u0 + i0 as f32 * self.res,
            v0: self.v0 + j0 as f32 * self.res,
            res: self.res,
            w,
            h,
            z,
            clear,
        }
    }

    /// The lane down `r`: from the middle of one short side, 2 m in, along the long side.
    pub fn lane(&self, r: Rect) -> Lane {
        let metres = self.res / UNITS_PER_METRE;
        let (wu, wv) = ((r.i1 - r.i0) as f32 * metres, (r.j1 - r.j0) as f32 * metres);
        let mid_u = self.u0 + (r.i0 + r.i1) as f32 * 0.5 * self.res;
        let mid_v = self.v0 + (r.j0 + r.j1) as f32 * 0.5 * self.res;
        let inset = 2.0 * UNITS_PER_METRE;
        let (start_u, start_v, du, dv, length, width) = if wu >= wv {
            (
                self.u0 + r.i0 as f32 * self.res + inset,
                mid_v,
                1.0,
                0.0,
                wu,
                wv,
            )
        } else {
            (
                mid_u,
                self.v0 + r.j0 as f32 * self.res + inset,
                0.0,
                1.0,
                wv,
                wu,
            )
        };
        let (x, y) = from_frame(self.angle, start_u, start_v);
        let (dx, dy) = from_frame(self.angle, du, dv);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for j in r.j0..r.j1 {
            for &z in &self.z[j * self.w + r.i0..j * self.w + r.i1] {
                if !z.is_nan() {
                    lo = lo.min(z);
                    hi = hi.max(z);
                }
            }
        }
        let z = self
            .at(x, y)
            .map_or(if lo <= hi { lo } else { 0.0 }, |(z, _)| z);
        Lane {
            start: [x, y, z],
            // The game's heading: around z, 0 north (+y), clockwise.
            heading: dx.atan2(dy).rem_euclid(std::f32::consts::TAU),
            length_m: length - 2.0,
            width_m: width,
            spread_m: if lo <= hi {
                (hi - lo) / UNITS_PER_METRE
            } else {
                0.0
            },
        }
    }
}

/// The world point (x, y) in the frame turned by `angle`.
pub fn to_frame(angle: f32, x: f32, y: f32) -> (f32, f32) {
    let (s, c) = angle.sin_cos();
    (x * c + y * s, -x * s + y * c)
}

/// The frame point (u, v) in the world.
pub fn from_frame(angle: f32, u: f32, v: f32) -> (f32, f32) {
    let (s, c) = angle.sin_cos();
    (u * c - v * s, u * s + v * c)
}

/// A rectangle's worth for the range, `length` and `width` in metres (the longer and the shorter
/// side): `None` below [`MIN_WIDTH`] or [`MIN_LENGTH`].
pub fn rect_score(length: f32, width: f32) -> Option<f32> {
    if width < MIN_WIDTH || length < MIN_LENGTH {
        return None;
    }
    let narrow = if width < GOOD_WIDTH {
        (width / GOOD_WIDTH).powi(2)
    } else {
        1.0
    };
    Some(length.min(LONG) / LONG * width.min(WIDE) / WIDE * narrow)
}

/// The best rectangle of squares with at least `min_clear` clearance ([`rect_score`]; among
/// equals the larger), with its score.
pub fn best_rect(floor: &Floor, min_clear: u16) -> Option<(Rect, f32)> {
    let (w, h) = (floor.w, floor.h);
    let metres = floor.res / UNITS_PER_METRE;
    let mut heights = vec![0usize; w];
    let mut best: Option<(Rect, f32, usize)> = None;
    let mut stack: Vec<(usize, usize)> = Vec::with_capacity(w + 1);
    for j in 0..h {
        for (i, height) in heights.iter_mut().enumerate() {
            *height = if floor.clear[j * w + i] >= min_clear {
                *height + 1
            } else {
                0
            };
        }
        stack.clear();
        for i in 0..=w {
            let height = heights.get(i).copied().unwrap_or(0);
            let mut start = i;
            while let Some(&(left, top)) = stack.last() {
                if top < height {
                    break;
                }
                stack.pop();
                // The rectangle `top` squares tall over [left, i).
                let (a, b) = ((i - left) as f32 * metres, top as f32 * metres);
                if let Some(score) = rect_score(a.max(b), a.min(b)) {
                    let area = (i - left) * top;
                    let better = best.as_ref().map_or(true, |&(_, s, ar)| {
                        score > s + 1e-6 || ((score - s).abs() <= 1e-6 && area > ar)
                    });
                    if better {
                        let rect = Rect {
                            i0: left,
                            j0: j + 1 - top,
                            i1: i,
                            j1: j + 1,
                        };
                        best = Some((rect, score, area));
                    }
                }
                start = left;
            }
            if height > 0 {
                stack.push((start, height));
            }
        }
    }
    best.map(|(r, s, _)| (r, s))
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: f32 = UNITS_PER_METRE;

    /// A flat rectangle of floor `w` by `l` metres from (x, y), as two triangles.
    fn slab(x: f32, y: f32, w: f32, l: f32, z: f32) -> Vec<[[f32; 3]; 3]> {
        let (x0, y0, x1, y1) = (x * M, y * M, (x + w) * M, (y + l) * M);
        vec![
            [[x0, y0, z], [x1, y0, z], [x1, y1, z]],
            [[x0, y0, z], [x1, y1, z], [x0, y1, z]],
        ]
    }

    /// A hall 20 m wide and 120 m long gives a lane down its length, the player at the south end
    /// facing north; a 4 m corridor beside it is not floor enough.
    #[test]
    fn a_wide_hall_gives_a_lane_and_a_corridor_none() {
        let mut tris = slab(0.0, 0.0, 20.0, 120.0, 0.0);
        tris.extend(slab(40.0, 0.0, 4.0, 150.0, 0.0));
        let floor = Floor::of(&tris, 0.0, M).unwrap();
        let (rect, score) = best_rect(&floor, 2).unwrap();
        let lane = floor.lane(rect);
        assert!((lane.width_m - 18.0).abs() <= 1.0, "{lane:?}");
        assert!(lane.length_m >= 110.0, "{lane:?}");
        assert!(lane.heading.abs() < 1e-3, "north: {lane:?}");
        assert!((lane.start[0] / M - 10.0).abs() <= 1.0, "{lane:?}");
        assert!(score > 0.5);
        let corridor = Floor::of(&slab(0.0, 0.0, 4.0, 150.0, 0.0), 0.0, M).unwrap();
        assert_eq!(best_rect(&corridor, 2), None);
    }

    /// Width counts as much as length: a 25 by 60 m hangar beats a 9 by 110 m gallery.
    #[test]
    fn width_beats_a_little_more_length() {
        assert!(rect_score(60.0, 25.0).unwrap() > rect_score(110.0, 9.0).unwrap());
        assert!(rect_score(110.0, 12.0).unwrap() > rect_score(110.0, 10.0).unwrap());
        assert_eq!(rect_score(200.0, 7.0), None);
        assert_eq!(rect_score(20.0, 20.0), None);
    }

    /// A walkway over the hall leaves its squares out; a steep ramp is not floor.
    #[test]
    fn overlapping_floors_and_slopes_are_left_out() {
        let mut tris = slab(0.0, 0.0, 30.0, 100.0, 0.0);
        tris.extend(slab(0.0, 40.0, 30.0, 3.0, 5.0 * M));
        let floor = Floor::of(&tris, 0.0, M).unwrap();
        assert!(floor.at(15.0 * M, 41.5 * M).is_none());
        assert!(floor.at(15.0 * M, 20.0 * M).is_some());
        let ramp = [[
            [0.0, 0.0, 0.0],
            [10.0 * M, 0.0, 0.0],
            [0.0, 10.0 * M, 5.0 * M],
        ]];
        let floor = Floor::of(&ramp, 0.0, M).unwrap();
        assert!(floor.at(2.0 * M, 2.0 * M).is_none());
    }

    /// The clearance grows a square at a time from the floor's edge; a spot's height comes back.
    #[test]
    fn the_clearance_is_the_distance_to_the_edge() {
        let floor = Floor::of(&slab(0.0, 0.0, 10.0, 10.0, 7.0), 0.0, M).unwrap();
        let (z, edge) = floor.at(0.5 * M, 5.0 * M).unwrap();
        assert_eq!(z, 7.0);
        assert!(edge <= 1.0, "{edge}");
        let (_, middle) = floor.at(5.0 * M, 5.0 * M).unwrap();
        assert!(middle >= 4.0, "{middle}");
    }

    /// A hall turned 45 degrees is found in the turned frame, the lane's heading along it.
    #[test]
    fn a_turned_hall_is_found_turned() {
        let angle = std::f32::consts::FRAC_PI_4;
        let tris: Vec<[[f32; 3]; 3]> = slab(0.0, 0.0, 16.0, 100.0, 0.0)
            .into_iter()
            .map(|t| {
                t.map(|p| {
                    let (x, y) = from_frame(angle, p[0], p[1]);
                    [x, y, p[2]]
                })
            })
            .collect();
        let straight = Floor::of(&tris, 0.0, M).unwrap();
        let turned = Floor::of(&tris, angle, M).unwrap();
        let s0 = best_rect(&straight, 2).map_or(0.0, |r| r.1);
        let (rect, s1) = best_rect(&turned, 2).unwrap();
        assert!(s1 > s0, "{s1} > {s0}");
        let lane = turned.lane(rect);
        // The hall runs along the frame's v axis: (-sin, cos) of 45 degrees, a heading of 315.
        assert!((lane.heading.to_degrees() - 315.0).abs() < 1.0, "{lane:?}");
    }

    /// An actor's surroundings are left out, and the rest of the floor still serves.
    #[test]
    fn an_actor_keeps_the_lane_away() {
        let mut floor = Floor::of(&slab(0.0, 0.0, 30.0, 300.0, 0.0), 0.0, M).unwrap();
        floor.exclude(15.0 * M, 50.0 * M, 40.0 * M);
        floor.clearance();
        assert!(floor.at(15.0 * M, 60.0 * M).is_none());
        let (rect, _) = best_rect(&floor, 2).unwrap();
        let lane = floor.lane(rect);
        assert!(lane.start[1] / M > 85.0, "{lane:?}");
    }

    /// A cropped grid answers as the whole one does.
    #[test]
    fn a_crop_answers_as_the_whole() {
        let floor = Floor::of(&slab(0.0, 0.0, 30.0, 100.0, 3.0), 0.0, M).unwrap();
        let (rect, _) = best_rect(&floor, 2).unwrap();
        let crop = floor.crop(
            Rect {
                i0: rect.i0 + 5,
                j0: rect.j0 + 5,
                i1: rect.i0 + 10,
                j1: rect.j0 + 10,
            },
            2,
        );
        for (x, y) in [(6.5, 6.5), (9.5, 12.5), (8.2, 7.4)] {
            assert_eq!(crop.at(x * M, y * M), floor.at(x * M, y * M), "{x}, {y}");
        }
        assert_eq!(crop.at(50.0 * M, 50.0 * M), None);
    }
}
