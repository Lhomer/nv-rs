//! Per-gun muzzle velocity multipliers (Patrick, 2026-11-14), for the ballistic bullets
//! ([`crate::ballistics`]): each gun's bullets leave at its calibre's velocity times this, on top
//! of the ammo load, barrel and suppressor mods and the game's projectile-speed multiplier.
//! Momentum, flight (and so drop) and penetration follow the speed.
//!
//! **The table** (`crates/host/data/gun_velocity.tsv`, [`TABLE`]): by the weapon's editor ID. The
//! game keeps no editor IDs in memory, so the official masters' `WEAP` records are read on a host
//! thread when the main menu is made ([`start_scan`]): each record of a plugin's own (not an
//! override of a master's) gives its form ID in the load order (the plugin's index, its local
//! ID) and its `EDID`. A plugin that edits a vanilla gun keeps its form ID, so its entry still
//! applies.
//!
//! **Without an entry** (a modded gun, or an entry whose editor ID is wrong): the gun's damage
//! against its calibre's average gun's, `(damage / average)^0.25`, within [`FALLBACK_RANGE`]
//! (the loaded guns of that calibre averaged once, [`fallback`]).
//!
//! **The report** (once, at the first shot after the scan): the table's entries that matched no
//! loaded weapon, and the calibre guns of the official masters without an entry (their editor
//! IDs). The table's editor IDs were read from the official masters (`data/official_weapons.tsv`,
//! which the tests check it against), so with the official masters alone both lists are empty.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// The table's text.
pub const TABLE_TEXT: &str = include_str!("../data/gun_velocity.tsv");
/// What a gun without an entry can get from its damage.
pub const FALLBACK_RANGE: (f32, f32) = (0.9, 1.1);
/// A multiplier from the table is held within this.
pub const TABLE_RANGE: (f32, f32) = (0.5, 2.0);

/// The table: editor ID (lower case) -> multiplier.
pub fn table() -> &'static HashMap<String, f32> {
    static TABLE: OnceLock<HashMap<String, f32>> = OnceLock::new();
    TABLE.get_or_init(|| parse(TABLE_TEXT))
}

/// The table file's rows: `editor id <tab> multiplier <tab> why`, `#` lines and blank lines left
/// out, a multiplier that is not a number left out.
pub fn parse(text: &str) -> HashMap<String, f32> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(id), Some(mult)) = (cols.next(), cols.next()) else {
            continue;
        };
        if let Ok(m) = mult.trim().parse::<f32>() {
            if m.is_finite() {
                map.insert(
                    id.trim().to_ascii_lowercase(),
                    m.clamp(TABLE_RANGE.0, TABLE_RANGE.1),
                );
            }
        }
    }
    map
}

/// Where a gun's multiplier came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// The table's entry for its editor ID.
    Table(String),
    /// Its damage against its calibre's average gun's.
    Damage { damage: f32, average: f32 },
    /// Nothing known: 1.0.
    Default,
}

/// The multiplier a gun of `damage` gets from its calibre's `average` damage.
pub fn from_damage(damage: f32, average: f32) -> f32 {
    if !(damage > 0.0 && average > 0.0) {
        return 1.0;
    }
    (damage / average)
        .powf(0.25)
        .clamp(FALLBACK_RANGE.0, FALLBACK_RANGE.1)
}

/// The official masters' weapons: form ID in the load order -> editor ID; `None` until read.
static EDITOR_IDS: Mutex<Option<HashMap<u32, String>>> = Mutex::new(None);
static SCAN_STARTED: AtomicBool = AtomicBool::new(false);

/// Reads the official masters' weapon editor IDs on a host thread, once per run (from the main
/// menu's making, `crate::gunplay::add_rows`).
pub fn start_scan(e: &engine::Engine) {
    if SCAN_STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let loaded = crate::cell_offsets::loaded_plugin_paths(e);
    let official = crate::range_cells::official_masters(&loaded);
    let jobs: Vec<(u32, std::path::PathBuf)> = official
        .into_iter()
        .filter_map(|p| loaded.iter().position(|l| l == &p).map(|i| (i as u32, p)))
        .collect();
    let spawned = std::thread::Builder::new()
        .name("nv-rs gun editor ids".into())
        .spawn(move || {
            let mut map = HashMap::new();
            for (index, path) in jobs {
                match read_weapons(&path, index) {
                    Ok(found) => map.extend(found),
                    Err(why) => eprintln!(
                        "Per-gun velocity: {} not read for editor IDs: {why}",
                        path.display()
                    ),
                }
            }
            eprintln!(
                "Per-gun velocity: {} weapons' editor IDs read from the official masters",
                map.len()
            );
            *EDITOR_IDS.lock().unwrap_or_else(|p| p.into_inner()) = Some(map);
        });
    if spawned.is_err() {
        eprintln!("Per-gun velocity: no thread for the editor IDs: the table is not used");
    }
}

/// A plugin's own weapons: (form ID in the load order, editor ID).
fn read_weapons(path: &std::path::Path, index: u32) -> esm::Result<Vec<(u32, String)>> {
    let plugin = esm::Plugin::open(path)?;
    let masters = plugin.header().masters.len() as u32;
    let mut found = Vec::new();
    for entry in plugin.records_of_type(esm::FourCC::new(b"WEAP")) {
        let id = entry.header.form_id.0;
        if id >> 24 < masters {
            continue; // an override of a master's weapon: that master gives it
        }
        if let Ok(Some(editor_id)) = plugin.editor_id_of(entry) {
            found.push(((index << 24) | (id & 0x00ff_ffff), editor_id));
        }
    }
    Ok(found)
}

/// The editor ID of the weapon whose form ID is `id`, once read.
pub fn editor_id(id: u32) -> Option<String> {
    EDITOR_IDS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()?
        .get(&id)
        .cloned()
}

/// The loaded guns' average damage by calibre name (made once, after the scan or at the first
/// ask).
static AVERAGES: Mutex<Option<HashMap<&'static str, f32>>> = Mutex::new(None);
static REPORTED: AtomicBool = AtomicBool::new(false);

/// Each calibre's average damage over the loaded guns with that calibre.
fn averages(e: &mut engine::Engine) -> HashMap<&'static str, f32> {
    if let Some(a) = AVERAGES.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        return a.clone();
    }
    let mut sums: HashMap<&'static str, (f32, u32)> = HashMap::new();
    for weapon in crate::test_range::all_forms(e) {
        if let Some((caliber, damage)) = crate::ballistics::gun_caliber(e, weapon) {
            let s = sums.entry(caliber.name).or_insert((0.0, 0));
            s.0 += damage;
            s.1 += 1;
        }
    }
    let a: HashMap<&'static str, f32> = sums
        .into_iter()
        .filter(|(_, (_, n))| *n > 0)
        .map(|(k, (s, n))| (k, s / n as f32))
        .collect();
    *AVERAGES.lock().unwrap_or_else(|p| p.into_inner()) = Some(a.clone());
    a
}

/// The calibres' average damage worked out ahead (from `crate::ballistics::apply_forms`).
pub fn prepare(e: &mut engine::Engine) {
    let _ = averages(e);
}

/// The multiplier of `weapon` (a form) and where it came from (module documentation).
pub fn multiplier(e: &mut engine::Engine, weapon: u32) -> (f32, Source) {
    if weapon == 0 {
        return (1.0, Source::Default);
    }
    let id = e.mem.u32(weapon + 0x0c);
    if let Some(editor_id) = editor_id(id) {
        report_once(e);
        if let Some(&m) = table().get(&editor_id.to_ascii_lowercase()) {
            return (m, Source::Table(editor_id));
        }
    }
    let Some((caliber, damage)) = crate::ballistics::gun_caliber(e, weapon) else {
        return (1.0, Source::Default);
    };
    match averages(e).get(caliber.name) {
        Some(&average) => (
            from_damage(damage, average),
            Source::Damage { damage, average },
        ),
        None => (1.0, Source::Default),
    }
}

/// Once, when the editor IDs are known: the table's entries that matched no loaded weapon, and
/// the official masters' calibre guns without an entry.
fn report_once(e: &mut engine::Engine) {
    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let ids: HashMap<u32, String> = EDITOR_IDS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .unwrap_or_default();
    let known: std::collections::HashSet<String> =
        ids.values().map(|s| s.to_ascii_lowercase()).collect();
    let mut unmatched: Vec<&String> = table().keys().filter(|k| !known.contains(*k)).collect();
    unmatched.sort();
    let mut missing: Vec<String> = Vec::new();
    let mut matched = 0;
    for (&id, editor_id) in &ids {
        let lookup = e.call(LOOKUP_FORM, &engine::args![id]).u32();
        if lookup == 0 || crate::ballistics::gun_caliber(e, lookup).is_none() {
            continue;
        }
        if table().contains_key(&editor_id.to_ascii_lowercase()) {
            matched += 1;
        } else {
            missing.push(editor_id.clone());
        }
    }
    missing.sort();
    eprintln!(
        "Per-gun velocity: {matched} calibre guns of the official masters from the table; {} \
         without an entry (their damage decides): {}; {} table entries match no weapon: {}",
        missing.len(),
        missing.join(", "),
        unmatched.len(),
        unmatched
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// `LookupFormByID`.
const LOOKUP_FORM: u32 = 0x0048_39c0;

#[cfg(test)]
mod tests {
    use super::*;

    /// The official masters' WEAP records: plugin, editor ID, animation type, ammo name.
    const OFFICIAL: &str = include_str!("../data/official_weapons.tsv");

    fn official() -> Vec<(&'static str, &'static str, i32, &'static str)> {
        OFFICIAL
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                (
                    f[0],
                    f[1],
                    f[2].parse().unwrap_or(-1),
                    f.get(3).copied().unwrap_or(""),
                )
            })
            .collect()
    }

    /// Every table entry is a WEAP of the official masters (Patrick's log, 2026-11-15: 39 of 61
    /// matched no weapon).
    #[test]
    fn every_entry_is_an_official_weapon() {
        let known: std::collections::HashSet<String> = official()
            .iter()
            .map(|r| r.1.to_ascii_lowercase())
            .collect();
        let mut missing: Vec<&String> = table().keys().filter(|k| !known.contains(*k)).collect();
        missing.sort();
        assert!(
            missing.is_empty(),
            "not in the official masters: {missing:?}"
        );
    }

    /// Every official calibre gun (a gun's animation, 3 to 9, firing an ammo of a calibre, as
    /// `crate::ballistics::gun_caliber` tells) has an entry.
    #[test]
    fn every_official_calibre_gun_has_an_entry() {
        let mut without: Vec<String> = official()
            .iter()
            .filter(|r| (3..=9).contains(&r.2))
            .filter(|r| ragdoll_core::rounds::caliber_of(r.3).is_some())
            .filter(|r| !table().contains_key(&r.1.to_ascii_lowercase()))
            .map(|r| format!("{} ({}, {})", r.1, r.0, r.3))
            .collect();
        without.sort();
        without.dedup();
        assert!(
            without.is_empty(),
            "calibre guns without an entry: {without:?}"
        );
    }

    /// The shipped table reads whole: every row a number within its range, editor IDs unique.
    #[test]
    fn the_table_reads() {
        let rows = TABLE_TEXT
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .count();
        let t = table();
        assert_eq!(
            t.len(),
            rows,
            "a row without a number or a repeated editor ID"
        );
        assert!(t.len() >= 50);
        for (id, &m) in t {
            assert!((0.8..=1.25).contains(&m), "{id} {m}");
        }
        assert_eq!(t.get("weapnvantimaterielrifle"), Some(&1.15));
        assert!(t["weapshotgunsawedoff"] < t["weapnvhuntingshotgun"]);
        let p = parse("# c\nA\t1.5\twhy\nB\tx\nC\t9\n");
        assert_eq!(p.get("a"), Some(&1.5));
        assert!(!p.contains_key("b"));
        assert_eq!(p.get("c"), Some(&TABLE_RANGE.1));
    }

    /// A gun without an entry: its damage against its calibre's average, within the range.
    #[test]
    fn the_fallback_follows_damage() {
        assert_eq!(from_damage(30.0, 30.0), 1.0);
        assert!(from_damage(40.0, 30.0) > 1.0);
        assert_eq!(from_damage(300.0, 30.0), FALLBACK_RANGE.1);
        assert_eq!(from_damage(1.0, 30.0), FALLBACK_RANGE.0);
        assert_eq!(from_damage(0.0, 30.0), 1.0);
        assert_eq!(from_damage(f32::NAN, 30.0), 1.0);
    }
}
