//! The world around a reacting body (Patrick, 2026-11-15): what its catch steps meet and what
//! it braces on. The host asks the game (its ray casts and navmesh), the lab Rapier, through
//! [`World`]; everything else is here, so both decide alike.
//!
//! - **Obstacles** ([`scan`]): level rays at several heights from the shin to the shoulders and
//!   rays down from above at steps ahead, each at three points across the body, so a railing or a
//!   table with gaps between its posts, rails or legs, or a top between two heights, is still
//!   found. The nearest hit gives the distance, a ray down just past it gives the top. By its height above the floor an obstacle is low (below the knee: it trips a step),
//!   waist-high (a railing, a table's edge, a half-wall: the hands brace on its top, and a big
//!   push folds the body over it) or a wall (the hands brace on its face).
//! - **The steps** ([`resolve`]): each planned catch step on the floor it lands on, or stopped:
//!   by a wall in its way (the step is shortened or not taken: a brace), by a ledge, or by a low
//!   obstacle its swinging foot does not clear: the foot catches and the body stumbles
//!   ([`Outcome::Trip`]). A stumble with little push left recovers with another step over the
//!   obstacle ([`recovery_step`]); with more it pitches over into the ragdoll ([`trips_over`]).
//! - **Folding over** ([`fold_launch`]): a body pushed hard into a waist-high obstacle goes over
//!   it, its hips lifted to the top and pivoting on it, before its ragdoll takes it.

use crate::balance::{classify, Ground, Outcome, UNITS_PER_METRE};
use crate::flinch::{add, cross, length, scale, sub, unit, V3};
use crate::tuning;

/// What the world answers a reacting body: the host's game, the lab's Rapier world.
pub trait World {
    /// The walkable floor under `at` (game units), if any near.
    fn floor(&mut self, at: V3) -> Option<V3>;
    /// How far from `from` along `dir` (unit) something solid is, within `range` (game units).
    /// Actors (the body itself among them) are not solid here.
    fn cast(&mut self, from: V3, dir: V3, range: f32) -> Option<f32>;
}

/// The heights (m over the floor, times the actor's scale) the obstacle rays go out at: shin,
/// knee, thigh, hip, waist, chest, shoulder.
pub const PROBE_HEIGHTS: [f32; 7] = [0.1, 0.22, 0.4, 0.6, 0.8, 1.0, 1.35];
/// Where across the body each height's rays start (m to its right, times the scale).
pub const PROBE_SPREAD: [f32; 3] = [-0.2, 0.0, 0.2];
/// Below this height (m, times the scale) an obstacle is low: a step trips on it; below
/// [`CHEST`] it is waist-high: braced on its top, folded over.
pub const KNEE: f32 = 0.45;
pub const CHEST: f32 = 1.25;
/// How high the shin rays for a step's path go out (m, times the scale).
pub const SHIN: [f32; 2] = [0.06, 0.16];
/// The rays down from above: from this height (m over the floor, times the scale), every
/// `DOWN_STEP` (game units) ahead; a surface higher than `DOWN_MIN` (game units) over the floor is
/// an obstacle's top.
pub const ABOVE: f32 = 2.0;
pub const DOWN_STEP: f32 = 4.0;
pub const DOWN_MIN: f32 = 3.0;
/// A wall in a step's way: the knee-height ray (game units over the foot) and the second, higher
/// one (a ramp or a stair the step goes up is met further on by it).
pub const WALL_KNEE: f32 = 25.0;
pub const WALL_HIGHER: f32 = 10.0;
/// A stumble keeps this share of the push; with more than [`TRIP_FALL`] (m/s) left it pitches
/// over the obstacle into the ragdoll, with less it steps over it.
pub const TRIP_KEEP: f32 = 0.7;
pub const TRIP_FALL: f32 = 0.9;
/// How far past the top of what it folds over the hips are lifted (m, times the scale).
pub const FOLD_CLEAR: f32 = 0.12;
/// How far short of an obstacle's face a foot may land (game units).
pub const FOOT_ROOM: f32 = 8.0;
/// How far ahead of a knocked-down body a waist-high obstacle makes it fold over (m, times the
/// scale).
pub const FOLD_REACH: f32 = 0.8;

/// What an obstacle is, by its height over the floor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Below the knee: a curb, debris, a low box.
    Low,
    /// Between the knee and the chest: a railing, a table's edge, a half-wall.
    Waist,
    /// Higher: a wall.
    Wall,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Low => "low obstacle",
            Kind::Waist => "waist-high obstacle",
            Kind::Wall => "wall",
        }
    }
}

/// An obstacle found ([`scan`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    /// How far ahead it is (game units, flat, from where the rays started).
    pub distance: f32,
    /// Its top (world z, game units) and its height over the floor (game units).
    pub top: f32,
    pub height: f32,
    pub kind: Kind,
    /// How many of the rays met it (of `PROBE_HEIGHTS × PROBE_SPREAD`): few for a railing.
    pub rays: u32,
}

/// The obstacle ahead of `at` along `dir` (flat) within `range` (game units), over the floor at
/// `floor_z`, for an actor of `size` (its scale; module documentation).
pub fn scan(
    world: &mut dyn World,
    at: V3,
    floor_z: f32,
    dir: V3,
    range: f32,
    size: f32,
) -> Option<Obstacle> {
    let dir = unit([dir[0], dir[1], 0.0])?;
    let side = cross(dir, [0.0, 0.0, 1.0]);
    let m = UNITS_PER_METRE * size.clamp(0.25, 4.0);
    let mut nearest: Option<(f32, V3)> = None;
    let mut highest = 0.0f32;
    let mut rays = 0;
    for h in PROBE_HEIGHTS {
        for o in PROBE_SPREAD {
            let from = add([at[0], at[1], floor_z + h * m], scale(side, o * m));
            if let Some(d) = world.cast(from, dir, range).filter(|d| d.is_finite()) {
                rays += 1;
                highest = highest.max(h);
                if nearest.map_or(true, |(n, _)| d < n) {
                    nearest = Some((d, from));
                }
            }
        }
    }
    // Rays down from above at steps ahead: a top between the heights above (a table's, a thin
    // rail's) is found by them.
    let above = floor_z + ABOVE * m;
    let mut top_down: Option<(f32, f32)> = None;
    let mut ahead = DOWN_STEP;
    while ahead <= range {
        for o in PROBE_SPREAD {
            let x = add(add(at, scale(dir, ahead)), scale(side, o * m));
            if let Some(d) =
                world.cast([x[0], x[1], above], [0.0, 0.0, -1.0], above - floor_z + 1.0)
            {
                let z = above - d;
                if z > floor_z + DOWN_MIN && z < above - 1.0 {
                    rays += 1;
                    if top_down.map_or(true, |(n, _)| ahead < n) {
                        top_down = Some((ahead, z));
                    }
                }
            }
        }
        if top_down.is_some() {
            break;
        }
        ahead += DOWN_STEP;
    }
    // The nearest of the two: a face met by a level ray, or a top met from above (its face a
    // step nearer at most).
    let (distance, from) = match (nearest, top_down) {
        (Some((d, f)), Some((a, _))) if d <= a => (d, f),
        (_, Some((a, _))) => ((a - DOWN_STEP * 0.5).max(0.0), [at[0], at[1], floor_z]),
        (Some(n), None) => n,
        (None, None) => return None,
    };
    // Its top: a ray down from well above, just past its face (a railing's rail is thin: two
    // tries), the top met from above, else the highest level ray that met it and a little.
    let top = [2.0, 6.0]
        .iter()
        .find_map(|past| {
            let x = add([from[0], from[1], 0.0], scale(dir, distance + past));
            world
                .cast([x[0], x[1], above], [0.0, 0.0, -1.0], above - floor_z + 1.0)
                .map(|d| above - d)
                .filter(|z| *z > floor_z + DOWN_MIN && *z < above - 1.0)
        })
        .or(top_down.map(|t| t.1));
    let top = top.unwrap_or(floor_z + (highest + 0.1) * m);
    let height = top - floor_z;
    let kind = if height < KNEE * m {
        Kind::Low
    } else if height < CHEST * m {
        Kind::Waist
    } else {
        Kind::Wall
    };
    Some(Obstacle {
        distance,
        top,
        height,
        kind,
        rays,
    })
}

/// Whether a wall stands between a foot at `from` and its step's target `to`: the ray at knee
/// height meets something, and a higher one meets it about as far off (a face near upright, not
/// a ramp or a stair the step goes up).
pub fn wall_between(world: &mut dyn World, from: V3, to: V3) -> bool {
    let d = [to[0] - from[0], to[1] - from[1], 0.0];
    let len = length(d);
    let Some(dir) = unit(d) else {
        return false;
    };
    let low = world.cast([from[0], from[1], from[2] + WALL_KNEE], dir, len + 8.0);
    let Some(low) = low else {
        return false;
    };
    let high = world.cast(
        [from[0], from[1], from[2] + WALL_KNEE + WALL_HIGHER],
        dir,
        len + 20.0,
    );
    high.is_some_and(|h| (h - low).abs() < 5.0)
}

/// A low obstacle in a step's path from `from` to `to` that the swinging foot (lifted the
/// tuning's swing height) does not clear: where the foot meets it and what it is.
pub fn trip_on(
    world: &mut dyn World,
    from: V3,
    to: V3,
    floor_z: f32,
    size: f32,
) -> Option<(V3, Obstacle)> {
    let d = [to[0] - from[0], to[1] - from[1], 0.0];
    let len = length(d);
    let dir = unit(d)?;
    let m = UNITS_PER_METRE * size.clamp(0.25, 4.0);
    let mut hit: Option<f32> = None;
    for h in SHIN {
        let start = [from[0], from[1], floor_z + h * m];
        if let Some(dist) = world.cast(start, dir, len + 6.0) {
            hit = Some(hit.map_or(dist, |x: f32| x.min(dist)));
        }
    }
    let dist = hit?;
    let above = floor_z + KNEE * m * 1.5;
    let past = add([from[0], from[1], 0.0], scale(dir, dist + 3.0));
    let top = world
        .cast(
            [past[0], past[1], above],
            [0.0, 0.0, -1.0],
            above - floor_z + 1.0,
        )
        .map(|x| above - x)
        .unwrap_or(above);
    let height = top - floor_z;
    let clear = tuning::get().swing_height * m * 0.8;
    if height <= clear || height >= KNEE * m {
        return None;
    }
    let caught = add(from, scale(dir, (dist - 4.0).max(0.0)));
    Some((
        [caught[0], caught[1], from[2]],
        Obstacle {
            distance: dist,
            top,
            height,
            kind: Kind::Low,
            rays: 1,
        },
    ))
}

/// The planned catch steps resolved against the world (module documentation).
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub steps: Vec<(usize, V3)>,
    pub outcome: Outcome,
    /// What stopped the steps, said in the log.
    pub said: Vec<&'static str>,
    /// The low obstacle a foot caught on.
    pub tripped: Option<Obstacle>,
}

/// Each planned step `planned` (foot, target) from the feet `feet` with their ankles `ankle`
/// over the floor (game units), on the floor it lands on, or stopped (module documentation);
/// `outcome` the plan's own (caught or overrun); `ahead` a waist-high obstacle or a wall the body
/// meets along the push (from [`scan`] at the body: where the rays started, the push, the
/// obstacle): no step lands past its face (a railing's gaps let the knee-height ray through,
/// not the body).
pub fn resolve(
    world: &mut dyn World,
    planned: &[(usize, V3)],
    feet: [V3; 2],
    ankle: [f32; 2],
    size: f32,
    outcome: Outcome,
    ahead: Option<(V3, V3, Obstacle)>,
) -> Resolved {
    let mut feet = feet;
    let mut out = Resolved {
        steps: Vec::new(),
        outcome,
        said: Vec::new(),
        tripped: None,
    };
    for &(foot, to) in planned {
        let from = feet[foot];
        let floor_z = from[2] - ankle[foot];
        if let Some((caught, obstacle)) = trip_on(world, from, to, floor_z, size) {
            out.steps.push((foot, caught));
            out.outcome = Outcome::Trip;
            out.tripped = Some(obstacle);
            out.said
                .push("a low obstacle: the swinging foot caught on it");
            break;
        }
        let past_it = ahead.is_some_and(|(at, dir, o)| {
            o.kind != Kind::Low
                && crate::flinch::dot(sub([to[0], to[1], 0.0], [at[0], at[1], 0.0]), dir)
                    > o.distance - FOOT_ROOM
        });
        let ground = if past_it || wall_between(world, from, to) {
            Ground::Blocked
        } else {
            classify(to, from[2], world.floor(to))
        };
        match ground {
            Ground::Floor(z) => {
                let to = [to[0], to[1], z + ankle[foot]];
                out.steps.push((foot, to));
                feet[foot] = to;
            }
            Ground::Ledge => {
                out.steps.push((foot, [to[0], to[1], from[2]]));
                out.outcome = Outcome::Ledge;
                out.said.push("a ledge");
                break;
            }
            Ground::Blocked => {
                let mut placed = false;
                for k in [0.66f32, 0.33] {
                    let short = add(from, scale(sub(to, from), k));
                    if let Ground::Floor(z) = classify(short, from[2], world.floor(short)) {
                        out.steps
                            .push((foot, [short[0], short[1], z + ankle[foot]]));
                        placed = true;
                        break;
                    }
                }
                out.outcome = Outcome::Wall;
                out.said.push(if placed {
                    "a wall: the step shortened"
                } else {
                    "a wall: no room to step"
                });
                break;
            }
        }
    }
    out
}

/// Whether a stumble (a foot caught at `v` m/s of push) pitches the body over the obstacle into
/// its ragdoll: more than [`TRIP_FALL`] left of it (less with a hurt leg's `weakness`, 0 to 1).
pub fn trips_over(v: f32, weakness: f32) -> bool {
    v * TRIP_KEEP > TRIP_FALL * (1.0 - 0.5 * weakness.clamp(0.0, 1.0))
}

/// The step that recovers from a stumble: the caught foot over the obstacle `obstacle`, landing
/// a foot's length past it along `dir`, on its top if it is wide enough to stand on, else on the
/// floor beyond (`None`: nowhere to land).
pub fn recovery_step(
    world: &mut dyn World,
    caught: V3,
    obstacle: &Obstacle,
    dir: V3,
    ankle: f32,
    size: f32,
) -> Option<V3> {
    let dir = unit([dir[0], dir[1], 0.0])?;
    let m = UNITS_PER_METRE * size.clamp(0.25, 4.0);
    let to = add(caught, scale(dir, 4.0 + 0.3 * m));
    let floor = world.floor([to[0], to[1], obstacle.top + 2.0])?;
    Some([to[0], to[1], floor[2] + ankle])
}

/// The ragdoll's launch for a body folding over a waist-high obstacle: along the push `dir` at
/// `speed` (m/s) and up enough that its hips, at `pelvis_z`, clear the top `top_z` (game units)
/// by [`FOLD_CLEAR`]: the direction (unit) and the speed.
pub fn fold_launch(dir: V3, speed: f32, pelvis_z: f32, top_z: f32, size: f32) -> (V3, f32) {
    let flat = unit([dir[0], dir[1], 0.0]).unwrap_or([0.0, 1.0, 0.0]);
    let m = UNITS_PER_METRE * size.clamp(0.25, 4.0);
    let rise = ((top_z - pelvis_z) / m + FOLD_CLEAR * size).max(0.05);
    let up = (2.0 * crate::balance::GRAVITY * rise).sqrt();
    let v = add(scale(flat, speed.max(0.5)), [0.0, 0.0, up]);
    let s = length(v);
    (scale(v, 1.0 / s.max(1e-6)), s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box world: axis-aligned solid boxes (game units) over a floor at z 0.
    struct Boxes(Vec<(V3, V3)>);

    impl World for Boxes {
        fn floor(&mut self, at: V3) -> Option<V3> {
            let top = self
                .0
                .iter()
                .filter(|(lo, hi)| {
                    (lo[0]..=hi[0]).contains(&at[0]) && (lo[1]..=hi[1]).contains(&at[1])
                })
                .map(|(_, hi)| hi[2])
                .filter(|z| *z <= at[2] + 30.0)
                .fold(0.0f32, f32::max);
            Some([at[0], at[1], top])
        }
        fn cast(&mut self, from: V3, dir: V3, range: f32) -> Option<f32> {
            let mut best: Option<f32> = None;
            let mut step = 0.0;
            while step <= range {
                let p = add(from, scale(dir, step));
                if p[2] <= 0.0 && dir[2] < 0.0 {
                    best = Some(step);
                    break;
                }
                if self
                    .0
                    .iter()
                    .any(|(lo, hi)| (0..3).all(|k| p[k] >= lo[k] && p[k] <= hi[k]))
                {
                    best = Some(step);
                    break;
                }
                step += 0.5;
            }
            best
        }
    }

    /// A railing (posts with gaps, a rail at 70 units), a half-wall, a table (a top at 52 on four
    /// legs) and a wall are told apart; the railing is found though most rays pass its gaps.
    #[test]
    fn obstacles_are_told_apart() {
        let dir = [0.0, -1.0, 0.0];
        let at = [0.0, 0.0, 66.0];
        let mut railing = Boxes(vec![
            ([-40.0, -26.0, 68.0], [40.0, -24.0, 70.0]),
            ([-21.0, -26.0, 0.0], [-19.0, -24.0, 70.0]),
            ([19.0, -26.0, 0.0], [21.0, -24.0, 70.0]),
        ]);
        let o = scan(&mut railing, at, 0.0, dir, 90.0, 1.0).expect("the railing");
        assert_eq!(o.kind, Kind::Waist);
        assert!((o.top - 70.0).abs() < 1.5, "{o:?}");
        assert!((o.distance - 24.0).abs() < 1.0, "{o:?}");
        let mut table = Boxes(vec![
            ([-40.0, -60.0, 49.0], [40.0, -25.0, 52.0]),
            ([-40.0, -28.0, 0.0], [-37.0, -25.0, 49.0]),
            ([37.0, -28.0, 0.0], [40.0, -25.0, 49.0]),
        ]);
        let o = scan(&mut table, at, 0.0, dir, 90.0, 1.0).expect("the table");
        assert_eq!(o.kind, Kind::Waist);
        assert!((o.top - 52.0).abs() < 1.5, "{o:?}");
        let mut half = Boxes(vec![([-100.0, -37.0, 0.0], [100.0, -25.0, 70.0])]);
        assert_eq!(
            scan(&mut half, at, 0.0, dir, 90.0, 1.0).unwrap().kind,
            Kind::Waist
        );
        let mut wall = Boxes(vec![([-100.0, -37.0, 0.0], [100.0, -25.0, 180.0])]);
        assert_eq!(
            scan(&mut wall, at, 0.0, dir, 90.0, 1.0).unwrap().kind,
            Kind::Wall
        );
        let mut curb = Boxes(vec![([-100.0, -80.0, 0.0], [100.0, -20.0, 15.0])]);
        assert_eq!(
            scan(&mut curb, at, 0.0, dir, 90.0, 1.0).unwrap().kind,
            Kind::Low
        );
        let mut nothing = Boxes(vec![]);
        assert!(scan(&mut nothing, at, 0.0, dir, 90.0, 1.0).is_none());
    }

    /// A step back onto a curb catches the foot; a step over flat floor does not; a step up a
    /// stair it clears is no trip.
    #[test]
    fn a_curb_trips_a_step() {
        let mut curb = Boxes(vec![([-100.0, -80.0, 0.0], [100.0, -20.0, 15.0])]);
        let from = [5.0, 0.0, 8.0];
        let to = [5.0, -35.0, 8.0];
        let (caught, o) = trip_on(&mut curb, from, to, 0.0, 1.0).expect("trips");
        assert!(caught[1] > -20.0 && caught[1] < 0.0, "{caught:?}");
        assert!((o.height - 15.0).abs() < 1.0);
        let mut flat = Boxes(vec![]);
        assert!(trip_on(&mut flat, from, to, 0.0, 1.0).is_none());
        let mut low = Boxes(vec![([-100.0, -80.0, 0.0], [100.0, -20.0, 2.0])]);
        assert!(trip_on(&mut low, from, to, 0.0, 1.0).is_none(), "cleared");
        let r = resolve(
            &mut curb,
            &[(0, to)],
            [from, [-5.0, 0.0, 8.0]],
            [8.0, 8.0],
            1.0,
            Outcome::Caught,
            None,
        );
        assert_eq!(r.outcome, Outcome::Trip);
        assert!(r.tripped.is_some());
        assert!(trips_over(2.0, 0.0) && !trips_over(0.8, 0.0));
        assert!(trips_over(1.1, 1.0), "a hurt leg trips over sooner");
    }

    /// Folding over a rail at 70 units from hips at 66: the launch lifts them clear.
    #[test]
    fn folding_over_lifts_the_hips() {
        let (d, s) = fold_launch([0.0, -1.0, 0.0], 2.0, 66.0, 70.0, 1.0);
        assert!(d[2] > 0.3 && d[1] < -0.5, "{d:?}");
        let up = d[2] * s;
        let rise = up * up / (2.0 * crate::balance::GRAVITY);
        assert!(rise * UNITS_PER_METRE >= 4.0 + FOLD_CLEAR * UNITS_PER_METRE - 0.5);
    }
}
