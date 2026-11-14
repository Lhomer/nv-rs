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
//! **What is chosen** ([`rank`]): a cell with a navmesh, no actors (nobody to fight, no quest NPC
//! to break), real architecture (at least 40 statics), light, and at least 20 m long; not one of
//! the editor's holding, storage or dummy cells. The longest wins (to 115 m, then the light
//! decides; the light only nudges a length, from 0.8 to 1.2 times), a test or QA cell of the
//! developers a little ahead. The ranked list is the order the
//! range tries.

use esm::{sig, FormId, FourCC, Plugin};
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
/// `CELL`'s `DATA` flag of an interior.
const INTERIOR: u8 = 1;

/// Measures every interior cell of `plugin` ([`CellStats`]).
pub fn scan(plugin: &Plugin) -> Vec<CellStats> {
    struct Children {
        positions: Vec<(f32, f32)>,
        statics: usize,
        actors: usize,
        navmesh: bool,
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
            navmesh: false,
        });
        if kind == NAVM {
            c.navmesh = true;
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
            Some(c) => (
                c.positions.len(),
                c.statics,
                c.actors,
                c.navmesh,
                length_of(&c.positions),
            ),
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
    cells
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

/// The cells fit for the range, best first ([`score`]).
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

/// A cell's fitness for the range, `None` for one that is not fit (module documentation).
pub fn score(c: &CellStats) -> Option<f32> {
    let name = c.editor_id.to_ascii_lowercase();
    let stash = ["holding", "storage", "dummy", "stash", "copy"]
        .iter()
        .any(|w| name.contains(w));
    if stash
        || !c.navmesh
        || c.actors > 0
        || c.statics < 40
        || c.light < 0.1
        || c.length < 20.0 * UNITS_PER_METRE
    {
        return None;
    }
    let long = c.length.min(115.0 * UNITS_PER_METRE);
    let test = name.contains("test") || name.starts_with("qa");
    Some(long * (0.8 + 0.4 * c.light.min(1.0)) * if test { 1.25 } else { 1.0 })
}

/// The base game's master in the load order: `FalloutNV.esm`'s path.
pub fn base_master(paths: &[std::path::PathBuf]) -> Option<&std::path::PathBuf> {
    paths.iter().find(|p| {
        let text = p.to_string_lossy();
        let name = text.rsplit(['\\', '/']).next().unwrap_or(&text);
        name.eq_ignore_ascii_case("FalloutNV.esm")
    })
}

/// The editor ids of the cells fit for the range in the plugin at `path`, best first, with a log
/// line each for the first few; empty when it cannot be read.
pub fn ranked_cells(path: &std::path::Path) -> Vec<String> {
    let plugin = match Plugin::open(path) {
        Ok(p) => p,
        Err(why) => {
            eprintln!("Test Range: {} not read ({why})", path.display());
            return Vec::new();
        }
    };
    let cells = scan(&plugin);
    let ranked = rank(&cells);
    for c in ranked.iter().take(5) {
        eprintln!(
            "Test Range: candidate {} ({:.0} m long, light {:.2}, {} statics)",
            c.editor_id,
            c.length / UNITS_PER_METRE,
            c.light,
            c.statics
        );
    }
    ranked.into_iter().map(|c| c.editor_id.clone()).collect()
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

    /// Only a walkable, quiet, lit, built and long cell is fit; the longest wins, the developers'
    /// test cells a little ahead, the light deciding among the long ones.
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
        let cells = [long, short, dark, busy, unwalkable, stash, bare, test, mid];
        let ranked: Vec<&str> = rank(&cells).iter().map(|c| c.editor_id.as_str()).collect();
        assert_eq!(ranked, ["NVTestHall", "WarehouseBig", "Hangar"]);
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

    #[test]
    fn the_length_leaves_strays_out() {
        let mut points: Vec<(f32, f32)> = (0..100).map(|i| (i as f32 * 10.0, 5.0)).collect();
        points.push((100_000.0, 0.0));
        let length = length_of(&points);
        assert!((850.0..1100.0).contains(&length), "{length}");
        assert_eq!(length_of(&[(1.0, 1.0)]), 0.0);
    }

    /// Against the player's own FalloutNV.esm (`NV_DATA`, the game's Data folder): the range has
    /// a place, its editor id is an interior cell of the file, and the last resort's worldspace is
    /// there too. Prints the ten best.
    ///
    /// ```text
    /// NV_DATA="C:\...\Fallout New Vegas\Data" cargo test -p host --lib range_cells -- --ignored --nocapture
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
        let cells = scan(&plugin);
        let ranked = rank(&cells);
        for c in ranked.iter().take(10) {
            println!(
                "{:40} {:5.0} m light {:.2} statics {:4} refs {:5}",
                c.editor_id,
                c.length / UNITS_PER_METRE,
                c.light,
                c.statics,
                c.refs
            );
        }
        let best = ranked.first().expect("a cell fit for the range");
        let entry = plugin
            .find_by_editor_id(&best.editor_id, Some(sig::CELL))
            .unwrap()
            .expect("the chosen cell is in the file");
        assert!(entry.world.is_none(), "an interior");
        assert!(best.length >= 20.0 * UNITS_PER_METRE);
        let world = crate::test_range::LAST_RESORT.name();
        let space = world.split(' ').next().unwrap();
        assert!(
            plugin
                .find_by_editor_id(space, Some(sig::WRLD))
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
