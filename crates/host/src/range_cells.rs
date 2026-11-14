//! Where the Test Range ([`crate::test_range`]) stands: the interior cells of the player's own
//! `FalloutNV.esm`, read with the plugin reader (`esm`) and ranked for a firing range, so the
//! choice never rests on a remembered cell name (the first version named `QASmoke`, a Fallout 3
//! cell FalloutNV.esm does not have; Patrick, 2026-11-14).
//!
//! **What is measured** ([`scan`]), for every interior cell (`CELL` outside a worldspace with the
//! interior flag) of the base game's master alone, so the choice is the same with or without DLC:
//! its editor id; its placed references (`REFR`) and how many of them are walls, floors and other
//! static architecture (`STAT`, `SCOL`, `MSTT`); its actors (`ACHR`, `ACRE`); whether it has a
//! navmesh (`NAVM`: a space made to be walked); its light (the cell's own `XCLL` ambient and
//! directional colours, or a lighting template); and its length: the longer side of the box the
//! middle 90 % of its references stand in (stray markers left out).
//!
//! **What is chosen** ([`rank`], then [`spots`]): a cell with a navmesh, no actors (nobody to
//! fight, no quest NPC to break), real architecture (at least 20 statics) and light; not one of
//! the editor's holding, storage or dummy cells, nor a passage, tunnel, corridor or sleep cell by
//! name. Then its floor decides ([`crate::range_floor`]): the widest, longest clear, flat
//! rectangle of its navmesh (tried square to the cell and at 45 degrees), at least 8 m wide and
//! 25 m long, width counting as much as length (Patrick, 097ad477: `NorthernPassageINT`, the
//! first choice by length, was a narrow passage whose walls the targets stood in). The light
//! nudges, a developers' test or QA cell a little ahead.
//!
//! **The Mojave too** ([`exterior_spot`]): the open, flat ground of `WastelandNV`, its navmesh laid
//! on one grid, every placed actor's surroundings (200 m) left out, so the place is away from
//! towns, camps and creatures; tried with the interiors by the same measure (a little behind).
//!
//! Each place comes with its lane ([`Spot`]): where the player stands, facing down the rectangle,
//! and the floor around it, which the range checks each target's spot against.

use crate::range_floor::{best_rect, Floor, Lane, Rect};
use esm::{sig, FormId, FourCC, Plugin, RecordEntry};
use std::collections::HashMap;

/// One interior cell's measures.
#[derive(Clone, Debug, PartialEq)]
pub struct CellStats {
    pub editor_id: String,
    pub refs: usize,
    pub statics: usize,
    pub actors: usize,
    pub navmesh: bool,
    /// The longer side of the box the middle 90 % of the references stand in, in game units.
    pub length: f32,
    /// 0 (dark) to 1 (the brightest ambient and directional light); a lighting template counts
    /// as [`TEMPLATE_LIGHT`].
    pub light: f32,
}

/// The light a cell lit by a lighting template (`LTMP`) counts as.
pub const TEMPLATE_LIGHT: f32 = 0.25;
/// Game units in a metre (one unit is about 1.43 cm).
pub const UNITS_PER_METRE: f32 = 70.0;

const NAVM: FourCC = FourCC::new(b"NAVM");
const STAT: FourCC = FourCC::new(b"STAT");
const SCOL: FourCC = FourCC::new(b"SCOL");
const MSTT: FourCC = FourCC::new(b"MSTT");
const XCLL: FourCC = FourCC::new(b"XCLL");
const LTMP: FourCC = FourCC::new(b"LTMP");
const NVVX: FourCC = FourCC::new(b"NVVX");
const NVTR: FourCC = FourCC::new(b"NVTR");
const XCLC: FourCC = FourCC::new(b"XCLC");
const WRLD: FourCC = FourCC::new(b"WRLD");
/// `CELL`'s `DATA` flag of an interior.
const INTERIOR: u8 = 1;

/// Measures every interior cell of `plugin` ([`CellStats`]).
pub fn scan(plugin: &Plugin) -> Vec<CellStats> {
    scan_with_navmeshes(plugin).0
}

/// [`scan`], with each cell's navmesh records by editor id.
fn scan_with_navmeshes(plugin: &Plugin) -> (Vec<CellStats>, HashMap<String, Vec<RecordEntry>>) {
    struct Children {
        positions: Vec<(f32, f32)>,
        statics: usize,
        actors: usize,
        navmesh: Vec<RecordEntry>,
    }
    let mut children: HashMap<FormId, Children> = HashMap::new();
    for entry in plugin.records() {
        let Some(cell) = entry.cell else {
            continue;
        };
        if entry.world.is_some() || entry.header.is_deleted() {
            continue;
        }
        let kind = entry.header.kind;
        if kind != sig::REFR && kind != sig::ACHR && kind != sig::ACRE && kind != NAVM {
            continue;
        }
        let c = children.entry(cell).or_insert(Children {
            positions: Vec::new(),
            statics: 0,
            actors: 0,
            navmesh: Vec::new(),
        });
        if kind == NAVM {
            c.navmesh.push(*entry);
        } else if kind != sig::REFR {
            c.actors += 1;
        } else {
            if let Ok(Some(data)) = plugin.subrecord_of(entry, sig::DATA) {
                if data.len() >= 8 {
                    let x = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                    let y = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                    if x.is_finite() && y.is_finite() {
                        c.positions.push((x, y));
                    }
                }
            }
            let base = plugin
                .subrecord_of(entry, sig::NAME)
                .ok()
                .flatten()
                .filter(|n| n.len() >= 4)
                .map(|n| FormId(u32::from_le_bytes([n[0], n[1], n[2], n[3]])));
            if let Some(base) = base.and_then(|b| plugin.get(b)) {
                let kind = base.header.kind;
                if kind == STAT || kind == SCOL || kind == MSTT {
                    c.statics += 1;
                }
            }
        }
    }
    let mut cells = Vec::new();
    let mut navmeshes = HashMap::new();
    for entry in plugin.records_of_type(sig::CELL) {
        if entry.world.is_some() || entry.header.is_deleted() {
            continue;
        }
        let interior = plugin
            .subrecord_of(entry, sig::DATA)
            .ok()
            .flatten()
            .is_some_and(|d| d.first().is_some_and(|f| f & INTERIOR != 0));
        if !interior {
            continue;
        }
        let Ok(Some(editor_id)) = plugin.editor_id_of(entry) else {
            continue;
        };
        let light = match plugin.subrecord_of(entry, XCLL).ok().flatten() {
            Some(x) if x.len() >= 8 => {
                let sum: u32 = [x[0], x[1], x[2], x[4], x[5], x[6]]
                    .iter()
                    .map(|&b| u32::from(b))
                    .sum();
                sum as f32 / (6.0 * 255.0)
            }
            _ if plugin.subrecord_of(entry, LTMP).ok().flatten().is_some() => TEMPLATE_LIGHT,
            _ => 0.0,
        };
        let c = children.remove(&entry.header.form_id);
        let (refs, statics, actors, navmesh, length) = match c {
            Some(c) => {
                let navmesh = !c.navmesh.is_empty();
                if navmesh {
                    navmeshes.insert(editor_id.clone(), c.navmesh);
                }
                (
                    c.positions.len(),
                    c.statics,
                    c.actors,
                    navmesh,
                    length_of(&c.positions),
                )
            }
            None => (0, 0, 0, false, 0.0),
        };
        cells.push(CellStats {
            editor_id,
            refs,
            statics,
            actors,
            navmesh,
            length,
            light,
        });
    }
    (cells, navmeshes)
}

/// The longer side of the box the middle 90 % of `positions` stand in (5th to 95th percentile
/// on each axis).
pub fn length_of(positions: &[(f32, f32)]) -> f32 {
    if positions.len() < 2 {
        return 0.0;
    }
    let span = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        let low = v[v.len() * 5 / 100];
        let high = v[(v.len() * 95 / 100).min(v.len() - 1)];
        high - low
    };
    let xs = positions.iter().map(|p| p.0).collect();
    let ys = positions.iter().map(|p| p.1).collect();
    span(xs).max(span(ys))
}

/// The cells fit for the range by their records, best first by length ([`score`]); their floor
/// decides after ([`spots`]).
pub fn rank(cells: &[CellStats]) -> Vec<&CellStats> {
    let mut fit: Vec<(&CellStats, f32)> = cells
        .iter()
        .filter_map(|c| score(c).map(|s| (c, s)))
        .collect();
    fit.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then_with(|| a.0.editor_id.cmp(&b.0.editor_id))
    });
    fit.into_iter().map(|(c, _)| c).collect()
}

/// Editor id words of the places the range never takes: the editor's holding cells, and the
/// narrow ones (Patrick: `NorthernPassageINT`, the vault's sleep cells).
const NOT_A_RANGE: [&str; 11] = [
    "holding",
    "storage",
    "dummy",
    "stash",
    "copy",
    "passage",
    "tunnel",
    "corridor",
    "sleepcell",
    "sewer",
    "vent",
];

/// A cell's fitness for the range by its records, `None` for one that is not fit (module
/// documentation); its length (to 115 m) nudged by its light.
pub fn score(c: &CellStats) -> Option<f32> {
    let name = c.editor_id.to_ascii_lowercase();
    if NOT_A_RANGE.iter().any(|w| name.contains(w))
        || !c.navmesh
        || c.actors > 0
        || c.statics < 20
        || c.light < 0.1
        || c.length < 20.0 * UNITS_PER_METRE
    {
        return None;
    }
    let long = c.length.min(115.0 * UNITS_PER_METRE);
    Some(long * (0.8 + 0.4 * c.light.min(1.0)))
}

/// Where the range runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    /// An interior cell, by editor id (`coc`).
    Interior(String),
    /// A worldspace's cell (`cow`).
    Exterior { space: String, x: i32, y: i32 },
}

/// A place for the range with its lane and the floor around it.
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub place: Place,
    pub lane: Lane,
    /// The floor around the lane, which each target's spot is checked against.
    pub floor: Floor,
    pub score: f32,
    /// Why it was chosen (for the log).
    pub why: String,
}

impl Spot {
    /// Whether the place has a lane from its floor (a place ranked by its records alone has
    /// none: [`spots`]).
    pub fn has_lane(&self) -> bool {
        self.lane.length_m > 0.0
    }

    pub fn name(&self) -> String {
        match &self.place {
            Place::Interior(cell) => cell.clone(),
            Place::Exterior { space, x, y } => format!("{space} {x},{y}"),
        }
    }
}

/// An exterior's score against an interior's of the same floor (the weather, the night, a
/// creature wandering in).
const EXTERIOR_FACTOR: f32 = 0.9;
/// The squares of clearance a lane's rectangle wants: a metre in from the navmesh's edge inside,
/// two squares (4 m) outside.
const INTERIOR_CLEAR: u16 = 2;
const EXTERIOR_CLEAR: u16 = 2;
/// Grid squares: 1 m inside, 2 m outside.
const INTERIOR_RES: f32 = UNITS_PER_METRE;
const EXTERIOR_RES: f32 = 2.0 * UNITS_PER_METRE;
/// How far from a placed actor an exterior lane stays, in units.
const ACTOR_DISTANCE: f32 = 200.0 * UNITS_PER_METRE;
/// The squares of floor a spot keeps around its rectangle.
const KEEP_MARGIN: usize = 12;
/// An exterior cell's side in units.
const CELL_UNITS: f32 = 4096.0;

/// A navmesh record's triangles, as world (or cell) points: `NVVX`'s vertices (three floats
/// each) and `NVTR`'s triangles (three vertex indices first; 16 bytes each in Fallout: New
/// Vegas, or the size `DATA`'s triangle count gives).
pub fn navmesh_triangles(plugin: &Plugin, entry: &RecordEntry) -> Vec<[[f32; 3]; 3]> {
    let Ok(subs) = plugin.subrecords(entry) else {
        return Vec::new();
    };
    let find = |k: FourCC| subs.iter().find(|s| s.kind == k).map(|s| s.data.as_slice());
    let (Some(vx), Some(tr)) = (find(NVVX), find(NVTR)) else {
        return Vec::new();
    };
    let f = |b: &[u8], at: usize| f32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
    let vertices: Vec<[f32; 3]> = vx
        .chunks_exact(12)
        .map(|c| [f(c, 0), f(c, 4), f(c, 8)])
        .collect();
    let counted = find(sig::DATA)
        .filter(|d| d.len() >= 12)
        .map(|d| u32::from_le_bytes([d[8], d[9], d[10], d[11]]) as usize)
        .filter(|&n| n > 0 && tr.len() % n == 0 && tr.len() / n >= 6);
    let stride = counted.map_or(16, |n| tr.len() / n);
    tr.chunks_exact(stride)
        .filter_map(|c| {
            let k = |at: usize| usize::from(u16::from_le_bytes([c[at], c[at + 1]]));
            let (a, b, d) = (k(0), k(2), k(4));
            let p = |i: usize| vertices.get(i).copied();
            let t = [p(a)?, p(b)?, p(d)?];
            t.iter()
                .all(|q| q.iter().all(|x| x.is_finite()))
                .then_some(t)
        })
        .collect()
}

/// An interior cell's lane: the best rectangle of its floor square to the cell or at 45
/// degrees, with its score (the light nudging, a test cell a little ahead).
pub fn interior_spot(c: &CellStats, triangles: &[[[f32; 3]; 3]]) -> Option<Spot> {
    let mut best: Option<(Floor, Rect, f32, f32)> = None;
    for angle in [0.0, std::f32::consts::FRAC_PI_4] {
        let Some(floor) = Floor::of(triangles, angle, INTERIOR_RES) else {
            continue;
        };
        if let Some((rect, score)) = best_rect(&floor, INTERIOR_CLEAR) {
            if best.as_ref().map_or(true, |b| score > b.2 + 1e-6) {
                best = Some((floor, rect, score, angle));
            }
        }
    }
    let (floor, rect, score, angle) = best?;
    let rect = trim(&floor, rect);
    let lane = floor.lane(rect);
    let name = c.editor_id.to_ascii_lowercase();
    let test = name.contains("test") || name.starts_with("qa");
    let score = score * (0.9 + 0.2 * c.light.min(1.0)) * if test { 1.1 } else { 1.0 }
        / (1.0 + lane.spread_m / 4.0);
    let why = format!(
        "{:.0} m wide, {:.0} m long clear flat floor ({:.0} degrees to the cell), level within {:.1} m, light {:.2}, {} statics, no actors",
        lane.width_m,
        lane.length_m,
        angle.to_degrees(),
        lane.spread_m,
        c.light,
        c.statics
    );
    Some(Spot {
        place: Place::Interior(c.editor_id.clone()),
        lane,
        floor: floor.crop(rect, KEEP_MARGIN),
        score,
        why,
    })
}

/// A rectangle cut to what the range uses: its long side to the lane's length (from its start),
/// its short side to the widest that counts, kept in the middle.
fn trim(floor: &Floor, r: Rect) -> Rect {
    let squares = |m: f32| ((m * UNITS_PER_METRE / floor.res).ceil() as usize).max(1);
    let long = squares(crate::range_floor::LONG + 4.0);
    let wide = squares(crate::range_floor::WIDE);
    let cut = |lo: usize, hi: usize, keep: usize, middle: bool| {
        if hi - lo <= keep {
            (lo, hi)
        } else if middle {
            let lo = lo + (hi - lo - keep) / 2;
            (lo, lo + keep)
        } else {
            (lo, lo + keep)
        }
    };
    let (wu, wv) = (r.i1 - r.i0, r.j1 - r.j0);
    let ((i0, i1), (j0, j1)) = if wu >= wv {
        (cut(r.i0, r.i1, long, false), cut(r.j0, r.j1, wide, true))
    } else {
        (cut(r.i0, r.i1, wide, true), cut(r.j0, r.j1, long, false))
    };
    Rect { i0, j0, i1, j1 }
}

/// The open ground of the worldspace `space`: its navmeshes on one grid of 2 m squares, every
/// placed actor's surroundings left out, the best rectangle; with the cell `cow` goes to.
pub fn exterior_spot(plugin: &Plugin, space: &str) -> Option<Spot> {
    let world = plugin
        .find_by_editor_id(space, Some(WRLD))
        .ok()??
        .header
        .form_id;
    let mut grid: HashMap<FormId, (i32, i32)> = HashMap::new();
    for entry in plugin.records_of_type(sig::CELL) {
        if entry.world != Some(world) {
            continue;
        }
        if let Ok(Some(d)) = plugin.subrecord_of(entry, XCLC) {
            if d.len() >= 8 {
                let x = i32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                let y = i32::from_le_bytes([d[4], d[5], d[6], d[7]]);
                grid.insert(entry.header.form_id, (x, y));
            }
        }
    }
    let meshes: Vec<&RecordEntry> = plugin
        .records_of_type(NAVM)
        .filter(|e| e.world == Some(world) && !e.header.is_deleted())
        .collect();
    let (mut lo, mut hi) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
    for m in &meshes {
        if let Some(&(x, y)) = m.cell.and_then(|c| grid.get(&c)) {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        }
    }
    if lo.0 > hi.0 {
        return None;
    }
    // A cell's navmesh can reach a little past its edge: a cell of margin all round.
    let (lo, hi) = ((lo.0 - 1, lo.1 - 1), (hi.0 + 1, hi.1 + 1));
    // 2 m squares, coarser for a worldspace whose grid would be very large.
    let cells = ((hi.0 - lo.0 + 1) as f32) * ((hi.1 - lo.1 + 1) as f32);
    let squares = cells * (CELL_UNITS / EXTERIOR_RES).powi(2);
    let res = if squares > 16.0e6 {
        EXTERIOR_RES * 2.0
    } else {
        EXTERIOR_RES
    };
    let mut floor = Floor::empty(
        0.0,
        res,
        lo.0 as f32 * CELL_UNITS,
        lo.1 as f32 * CELL_UNITS,
        (hi.0 + 1) as f32 * CELL_UNITS,
        (hi.1 + 1) as f32 * CELL_UNITS,
    );
    let mut layered = vec![false; floor.z.len()];
    for m in &meshes {
        for t in navmesh_triangles(plugin, m) {
            floor.lay(&t, &mut layered);
        }
    }
    for (z, &l) in floor.z.iter_mut().zip(&layered) {
        if l {
            *z = f32::NAN;
        }
    }
    drop(layered);
    let mut actors = 0;
    for entry in plugin.records() {
        let kind = entry.header.kind;
        if entry.world != Some(world)
            || entry.header.is_deleted()
            || (kind != sig::ACHR && kind != sig::ACRE)
        {
            continue;
        }
        if let Ok(Some(d)) = plugin.subrecord_of(entry, sig::DATA) {
            if d.len() >= 8 {
                let x = f32::from_le_bytes([d[0], d[1], d[2], d[3]]);
                let y = f32::from_le_bytes([d[4], d[5], d[6], d[7]]);
                if x.is_finite() && y.is_finite() {
                    floor.exclude(x, y, ACTOR_DISTANCE);
                    actors += 1;
                }
            }
        }
    }
    floor.clearance();
    let (rect, score) = best_rect(&floor, EXTERIOR_CLEAR)?;
    let rect = trim(&floor, rect);
    let lane = floor.lane(rect);
    let cell = |u: f32| (u / CELL_UNITS).floor() as i32;
    let why = format!(
        "{:.0} m wide, {:.0} m long clear flat ground, level within {:.1} m, 200 m or more from each of the {actors} placed actors",
        lane.width_m, lane.length_m, lane.spread_m
    );
    Some(Spot {
        place: Place::Exterior {
            space: space.to_string(),
            x: cell(lane.start[0]),
            y: cell(lane.start[1]),
        },
        lane,
        floor: floor.crop(rect, KEEP_MARGIN),
        score: score * EXTERIOR_FACTOR / (1.0 + lane.spread_m / 4.0),
        why,
    })
}

/// The places for the range in `plugin`, best first: the fit interiors by their floor and the
/// Mojave's open ground.
pub fn spots(plugin: &Plugin) -> Vec<Spot> {
    let (cells, navmeshes) = scan_with_navmeshes(plugin);
    let ranked = rank(&cells);
    let mut found: Vec<Spot> = ranked
        .iter()
        .filter_map(|c| {
            let meshes = navmeshes.get(&c.editor_id)?;
            let triangles: Vec<[[f32; 3]; 3]> = meshes
                .iter()
                .flat_map(|m| navmesh_triangles(plugin, m))
                .collect();
            interior_spot(c, &triangles)
        })
        .collect();
    if found.is_empty() && !ranked.is_empty() {
        // No floor read from any navmesh (a file the reader does not understand): the cells by
        // their records, as before, without a lane.
        eprintln!("Test Range: no navmesh floor could be measured: the cells are ranked by length");
        found.extend(ranked.iter().take(4).map(|c| Spot {
            place: Place::Interior(c.editor_id.clone()),
            lane: Lane {
                start: [0.0; 3],
                heading: 0.0,
                length_m: 0.0,
                width_m: 0.0,
                spread_m: 0.0,
            },
            floor: Floor::empty(0.0, INTERIOR_RES, 0.0, 0.0, 0.0, 0.0),
            score: 0.0,
            why: format!(
                "{:.0} m long by its references (no floor measured)",
                c.length / UNITS_PER_METRE
            ),
        }));
    }
    found.extend(exterior_spot(plugin, "WastelandNV"));
    found.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.name().cmp(&b.name()))
    });
    found
}

/// The base game's master in the load order: `FalloutNV.esm`'s path.
pub fn base_master(paths: &[std::path::PathBuf]) -> Option<&std::path::PathBuf> {
    paths.iter().find(|p| {
        let text = p.to_string_lossy();
        let name = text.rsplit(['\\', '/']).next().unwrap_or(&text);
        name.eq_ignore_ascii_case("FalloutNV.esm")
    })
}

/// The places for the range in the plugin at `path`, best first ([`spots`]), with a log line
/// each for the first few; empty when it cannot be read.
pub fn ranked_spots(path: &std::path::Path) -> Vec<Spot> {
    let plugin = match Plugin::open(path) {
        Ok(p) => p,
        Err(why) => {
            eprintln!("Test Range: {} not read ({why})", path.display());
            return Vec::new();
        }
    };
    let found = spots(&plugin);
    for s in found.iter().take(6) {
        eprintln!(
            "Test Range: candidate {} (score {:.2}): {}",
            s.name(),
            s.score,
            s.why
        );
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(name: &str, metres: f32, light: f32) -> CellStats {
        CellStats {
            editor_id: name.to_string(),
            refs: 300,
            statics: 200,
            actors: 0,
            navmesh: true,
            length: metres * UNITS_PER_METRE,
            light,
        }
    }

    /// Only a walkable, quiet, lit, built and long cell is fit, and never a passage or a sleep
    /// cell by name; by its records the longest comes first, the light deciding among the long
    /// ones (the floor decides after: [`spots`]).
    #[test]
    fn the_range_wants_a_long_quiet_lit_walkable_hall() {
        let long = cell("WarehouseBig", 90.0, 0.4);
        let short = cell("Shack", 12.0, 0.6);
        let dark = cell("Cave", 100.0, 0.02);
        let busy = CellStats {
            actors: 3,
            ..cell("Casino", 120.0, 0.6)
        };
        let unwalkable = CellStats {
            navmesh: false,
            ..cell("Void", 120.0, 0.6)
        };
        let stash = cell("AAAHoldingCell", 120.0, 0.6);
        let bare = CellStats {
            statics: 3,
            ..cell("Bare", 120.0, 0.6)
        };
        let test = cell("NVTestHall", 80.0, 0.4);
        let mid = cell("Hangar", 60.0, 0.9);
        let passage = cell("NorthernPassageINT", 120.0, 0.6);
        let sleep = cell("OVSleepCell01", 30.0, 0.6);
        let cells = [
            long, short, dark, busy, unwalkable, stash, bare, test, mid, passage, sleep,
        ];
        let ranked: Vec<&str> = rank(&cells).iter().map(|c| c.editor_id.as_str()).collect();
        assert_eq!(ranked, ["WarehouseBig", "NVTestHall", "Hangar"]);
        // Beyond 115 m the light decides.
        let a = cell("A", 200.0, 0.3);
        let b = cell("B", 130.0, 0.5);
        assert!(score(&b).unwrap() > score(&a).unwrap());
    }

    fn sub(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = kind.to_vec();
        out.extend((data.len() as u16).to_le_bytes());
        out.extend(data);
        out
    }

    fn record(kind: &[u8; 4], form_id: u32, data: &[u8]) -> Vec<u8> {
        let mut out = kind.to_vec();
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(form_id.to_le_bytes());
        out.extend([0u8; 8]);
        out.extend(data);
        out
    }

    fn group(label: [u8; 4], kind: i32, body: &[u8]) -> Vec<u8> {
        let mut out = b"GRUP".to_vec();
        out.extend((body.len() as u32 + 24).to_le_bytes());
        out.extend(label);
        out.extend(kind.to_le_bytes());
        out.extend([0u8; 8]);
        out.extend(body);
        out
    }

    fn reference(x: f32, base: u32, form_id: u32) -> Vec<u8> {
        let mut data = [0u8; 24];
        data[..4].copy_from_slice(&x.to_le_bytes());
        let mut body = sub(b"NAME", &base.to_le_bytes());
        body.extend(sub(b"DATA", &data));
        record(b"REFR", form_id, &body)
    }

    /// A cell made of the records the reader indexes: its editor id, interior flag, light, its
    /// references' spread and statics, its actors and its navmesh are measured.
    fn plugin(cells: &[(&str, u32, f32, usize, bool, bool)]) -> Plugin {
        let mut hedr = 1.34f32.to_le_bytes().to_vec();
        hedr.extend([0u8; 8]);
        let mut bytes = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        bytes.extend(group(
            *b"STAT",
            0,
            &record(b"STAT", 0x100, &sub(b"EDID", b"Wall\0")),
        ));
        let mut block = Vec::new();
        for &(name, id, metres, statics, navmesh, actor) in cells {
            let mut edid = name.as_bytes().to_vec();
            edid.push(0);
            let mut body = sub(b"EDID", &edid);
            body.extend(sub(b"DATA", &[1]));
            let mut light = vec![0u8; 40];
            light[..3].copy_from_slice(&[200, 200, 200]);
            light[4..7].copy_from_slice(&[200, 200, 200]);
            body.extend(sub(b"XCLL", &light));
            block.extend(record(b"CELL", id, &body));
            let mut children = Vec::new();
            for k in 0..statics {
                let x = metres * UNITS_PER_METRE * k as f32 / (statics - 1) as f32;
                children.extend(reference(x, 0x100, id + 1 + k as u32));
            }
            if navmesh {
                children.extend(record(b"NAVM", id + 0x900, &[]));
            }
            if actor {
                children.extend(record(
                    b"ACHR",
                    id + 0x901,
                    &sub(b"NAME", &7u32.to_le_bytes()),
                ));
            }
            let label = id.to_le_bytes();
            block.extend(group(label, 6, &group(label, 9, &children)));
        }
        bytes.extend(group(
            *b"CELL",
            0,
            &group([0; 4], 2, &group([0; 4], 3, &block)),
        ));
        Plugin::from_bytes(bytes).expect("the made plugin")
    }

    #[test]
    fn the_scan_measures_each_interior_cell() {
        let plugin = plugin(&[
            ("LongHall", 0x1000, 80.0, 60, true, false),
            ("Shed", 0x2000, 8.0, 60, true, false),
            ("Barracks", 0x3000, 90.0, 60, true, true),
            ("NoMesh", 0x4000, 90.0, 60, false, false),
        ]);
        let cells = scan(&plugin);
        let hall = cells.iter().find(|c| c.editor_id == "LongHall").unwrap();
        assert_eq!((hall.refs, hall.statics, hall.actors), (60, 60, 0));
        assert!(hall.navmesh);
        assert!((hall.light - 200.0 / 255.0).abs() < 1e-3, "{}", hall.light);
        let metres = hall.length / UNITS_PER_METRE;
        assert!((65.0..=80.0).contains(&metres), "{metres}");
        let barracks = cells.iter().find(|c| c.editor_id == "Barracks").unwrap();
        assert_eq!(barracks.actors, 1);
        let ranked: Vec<&str> = rank(&cells).iter().map(|c| c.editor_id.as_str()).collect();
        assert_eq!(ranked, ["LongHall"]);
    }

    /// A navmesh record of flat floor `w` by `l` metres at (x, y) (two triangles), as the game
    /// writes it: `DATA` (cell, counts), `NVVX`, `NVTR` (16 bytes a triangle).
    fn navmesh(id: u32, x: f32, y: f32, w: f32, l: f32) -> Vec<u8> {
        let m = UNITS_PER_METRE;
        let corners = [(x, y), (x + w, y), (x + w, y + l), (x, y + l)];
        let mut vx = Vec::new();
        for (cx, cy) in corners {
            for v in [cx * m, cy * m, 0.0f32] {
                vx.extend(v.to_le_bytes());
            }
        }
        let mut tr = Vec::new();
        for t in [[0u16, 1, 2], [0, 2, 3]] {
            for k in t {
                tr.extend(k.to_le_bytes());
            }
            tr.extend([0xffu8; 6]);
            tr.extend([0u8; 4]);
        }
        let mut data = 0u32.to_le_bytes().to_vec();
        data.extend(4u32.to_le_bytes());
        data.extend(2u32.to_le_bytes());
        data.extend([0u8; 12]);
        let mut body = sub(b"DATA", &data);
        body.extend(sub(b"NVVX", &vx));
        body.extend(sub(b"NVTR", &tr));
        record(b"NAVM", id, &body)
    }

    /// Two interiors with the same records and different floors: the wide hall is the range,
    /// the narrow gallery is not, and the hall's lane runs down its length.
    #[test]
    fn the_floor_decides_between_a_hall_and_a_gallery() {
        let mut hedr = 1.34f32.to_le_bytes().to_vec();
        hedr.extend([0u8; 8]);
        let mut bytes = record(b"TES4", 0, &sub(b"HEDR", &hedr));
        bytes.extend(group(
            *b"STAT",
            0,
            &record(b"STAT", 0x100, &sub(b"EDID", b"Wall\0")),
        ));
        let mut block = Vec::new();
        for (name, id, width) in [("Gallery", 0x1000u32, 5.0f32), ("Hall", 0x2000, 24.0)] {
            let mut edid = name.as_bytes().to_vec();
            edid.push(0);
            let mut body = sub(b"EDID", &edid);
            body.extend(sub(b"DATA", &[1]));
            let mut light = vec![0u8; 40];
            light[..3].copy_from_slice(&[200, 200, 200]);
            body.extend(sub(b"XCLL", &light));
            block.extend(record(b"CELL", id, &body));
            let mut children = Vec::new();
            for k in 0..40u32 {
                let x = 120.0 * UNITS_PER_METRE * k as f32 / 39.0;
                children.extend(reference(x, 0x100, id + 1 + k));
            }
            children.extend(navmesh(id + 0x900, 0.0, 0.0, width, 120.0));
            let label = id.to_le_bytes();
            block.extend(group(label, 6, &group(label, 9, &children)));
        }
        bytes.extend(group(
            *b"CELL",
            0,
            &group([0; 4], 2, &group([0; 4], 3, &block)),
        ));
        let plugin = Plugin::from_bytes(bytes).expect("the made plugin");
        let found = spots(&plugin);
        let names: Vec<String> = found.iter().map(Spot::name).collect();
        assert_eq!(names, ["Hall"]);
        let lane = found[0].lane;
        assert!(lane.width_m >= 20.0 && lane.length_m >= 100.0, "{lane:?}");
        assert!(lane.heading.abs() < 1e-3, "{lane:?}");
        assert!(found[0].why.contains("m wide"), "{}", found[0].why);
        // The kept floor answers for the lane's targets.
        let (x, y) = (lane.start[0], lane.start[1] + 50.0 * UNITS_PER_METRE);
        assert!(found[0].floor.at(x, y).is_some());
    }

    /// The Mojave's open ground: a flat stretch of `WastelandNV`'s navmesh gives a lane, and the
    /// cell `cow` goes to; an actor placed on it keeps the range 200 m away (here: none left).
    #[test]
    fn the_mojave_gives_open_ground_away_from_actors() {
        let made = |actor: bool| {
            let mut hedr = 1.34f32.to_le_bytes().to_vec();
            hedr.extend([0u8; 8]);
            let mut bytes = record(b"TES4", 0, &sub(b"HEDR", &hedr));
            let mut world = record(b"WRLD", 0x10, &sub(b"EDID", b"WastelandNV\0"));
            let mut cell = sub(b"DATA", &[0]);
            let mut grid = 1i32.to_le_bytes().to_vec();
            grid.extend((-2i32).to_le_bytes());
            cell.extend(sub(b"XCLC", &grid));
            let mut children = navmesh(0x900, 60.0, -150.0, 40.0, 140.0);
            if actor {
                let mut data = [0u8; 24];
                data[..4].copy_from_slice(&(80.0 * UNITS_PER_METRE).to_le_bytes());
                data[4..8].copy_from_slice(&(-80.0 * UNITS_PER_METRE).to_le_bytes());
                let mut body = sub(b"NAME", &7u32.to_le_bytes());
                body.extend(sub(b"DATA", &data));
                children.extend(record(b"ACRE", 0x901, &body));
            }
            let label = 0x20u32.to_le_bytes();
            let mut cells = record(b"CELL", 0x20, &cell);
            cells.extend(group(label, 6, &group(label, 9, &children)));
            let blocks = group([0; 4], 4, &group([0; 4], 5, &cells));
            world.extend(group(0x10u32.to_le_bytes(), 1, &blocks));
            bytes.extend(group(*b"WRLD", 0, &world));
            Plugin::from_bytes(bytes).expect("the made plugin")
        };
        let spot = exterior_spot(&made(false), "WastelandNV").expect("open ground");
        assert!(
            spot.lane.width_m >= 20.0 && spot.lane.length_m >= 100.0,
            "{:?}",
            spot.lane
        );
        // `cow` goes to the cell of the lane's start (4096 units a cell).
        let cell = |u: f32| (u / 4096.0).floor() as i32;
        assert_eq!(
            spot.place,
            Place::Exterior {
                space: "WastelandNV".into(),
                x: cell(spot.lane.start[0]),
                y: cell(spot.lane.start[1])
            }
        );
        assert_eq!(cell(spot.lane.start[0]), 1);
        assert!(spot.why.contains("placed actors"), "{}", spot.why);
        assert_eq!(exterior_spot(&made(true), "WastelandNV"), None);
        assert_eq!(exterior_spot(&made(false), "NoSuchWorld"), None);
    }

    #[test]
    fn the_length_leaves_strays_out() {
        let mut points: Vec<(f32, f32)> = (0..100).map(|i| (i as f32 * 10.0, 5.0)).collect();
        points.push((100_000.0, 0.0));
        let length = length_of(&points);
        assert!((850.0..1100.0).contains(&length), "{length}");
        assert_eq!(length_of(&[(1.0, 1.0)]), 0.0);
    }

    /// Against the player's own FalloutNV.esm (`NV_DATA`, the game's Data folder): the range has
    /// a place, wide and long enough, and an interior's editor id is a cell of the file; the last
    /// resort's worldspace is there too. Prints the ten best.
    ///
    /// ```text
    /// NV_DATA="C:\...\Fallout New Vegas\Data" cargo test --release -p host --lib range_cells -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs the player's FalloutNV.esm (set NV_DATA)"]
    fn the_range_has_a_place_in_the_real_falloutnv_esm() {
        let Some(data) = std::env::var_os("NV_DATA") else {
            eprintln!("NV_DATA is not set: skipped");
            return;
        };
        let path = std::path::Path::new(&data).join("FalloutNV.esm");
        let plugin = Plugin::open(&path).expect("FalloutNV.esm");
        let found = spots(&plugin);
        for s in found.iter().take(10) {
            println!("{:32} {:.2} {}", s.name(), s.score, s.why);
        }
        let best = found.first().expect("a place fit for the range");
        assert!(best.lane.width_m >= crate::range_floor::MIN_WIDTH);
        if let Place::Interior(cell) = &best.place {
            let entry = plugin
                .find_by_editor_id(cell, Some(sig::CELL))
                .unwrap()
                .expect("the chosen cell is in the file");
            assert!(entry.world.is_none(), "an interior");
        }
        let world = crate::test_range::last_resort().name();
        let space = world.split(' ').next().unwrap();
        assert!(
            plugin
                .find_by_editor_id(space, Some(WRLD))
                .unwrap()
                .is_some(),
            "{space} is in the file"
        );
    }

    #[test]
    fn the_base_master_is_found_by_name() {
        let paths = [
            std::path::PathBuf::from("C:\\Game\\Data\\DeadMoney.esm"),
            std::path::PathBuf::from("C:\\Game\\Data\\FALLOUTNV.ESM"),
        ];
        assert_eq!(base_master(&paths), Some(&paths[1]));
        assert_eq!(base_master(&paths[..1]), None);
    }
}
