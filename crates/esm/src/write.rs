//! A writer for the same format, enough for a small plugin of nv-rs's own (`host::perks`: five
//! perk records made in code, never shipped as a file): subrecords, records, a top group, the
//! `TES4` header, and a condition (`CTDA`). Fallout: New Vegas' layout: a record's header is 24
//! bytes (type, data size, flags, form id, version control, form version 15, unknown), a group's
//! too (`GRUP`, its whole size, the label, type 0 for a top group, stamp and version), a
//! subrecord's 6 (type, a 16-bit size).

/// Fallout: New Vegas' record form version.
pub const FORM_VERSION: u16 = 15;
/// `HEDR`'s version for Fallout: New Vegas plugins.
pub const HEADER_VERSION: f32 = 1.34;

/// A subrecord: its type and data (at most 65,535 bytes).
pub fn subrecord(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    assert!(
        data.len() <= u16::MAX as usize,
        "a subrecord's data is at most 65,535 bytes"
    );
    let mut out = Vec::with_capacity(6 + data.len());
    out.extend_from_slice(kind);
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// A zero-terminated string subrecord (`EDID`, `FULL`, `DESC`, `ICON`).
pub fn text(kind: &[u8; 4], value: &str) -> Vec<u8> {
    let mut data = value.as_bytes().to_vec();
    data.push(0);
    subrecord(kind, &data)
}

/// A record: its type, form id and subrecords (already made), no flags.
pub fn record(kind: &[u8; 4], form_id: u32, subrecords: &[Vec<u8>]) -> Vec<u8> {
    let data: Vec<u8> = subrecords.concat();
    let mut out = Vec::with_capacity(24 + data.len());
    out.extend_from_slice(kind);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&form_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&FORM_VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// A top group of the records of type `label` (already made).
pub fn group(label: &[u8; 4], records: &[Vec<u8>]) -> Vec<u8> {
    let data: Vec<u8> = records.concat();
    let mut out = Vec::with_capacity(24 + data.len());
    out.extend_from_slice(b"GRUP");
    out.extend_from_slice(&((24 + data.len()) as u32).to_le_bytes());
    out.extend_from_slice(label);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&data);
    out
}

/// The `TES4` header record: `HEDR` (version, the number of records and groups, the next object
/// id), the author (`CNAM`) and the description (`SNAM`); no masters.
pub fn header(records: u32, next_object: u32, author: &str, description: &str) -> Vec<u8> {
    let mut hedr = Vec::with_capacity(12);
    hedr.extend_from_slice(&HEADER_VERSION.to_le_bytes());
    hedr.extend_from_slice(&records.to_le_bytes());
    hedr.extend_from_slice(&next_object.to_le_bytes());
    record(
        b"TES4",
        0,
        &[
            subrecord(b"HEDR", &hedr),
            text(b"CNAM", author),
            text(b"SNAM", description),
        ],
    )
}

/// A condition's comparison (`CTDA`'s type byte, its top three bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    Equal = 0,
    NotEqual = 1,
    Greater = 2,
    GreaterOrEqual = 3,
    Less = 4,
    LessOrEqual = 5,
}

/// A condition (`CTDA`, 28 bytes): `function` (the condition function's index) with its two
/// parameters, compared with `value`, run on the subject.
pub fn condition(function: u16, param1: u32, param2: u32, compare: Compare, value: f32) -> Vec<u8> {
    let mut data = Vec::with_capacity(28);
    data.push((compare as u8) << 5);
    data.extend_from_slice(&[0u8; 3]);
    data.extend_from_slice(&value.to_le_bytes());
    data.extend_from_slice(&function.to_le_bytes());
    data.extend_from_slice(&[0u8; 2]);
    data.extend_from_slice(&param1.to_le_bytes());
    data.extend_from_slice(&param2.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    subrecord(b"CTDA", &data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{sig, FourCC, Plugin};

    /// A plugin made here reads back with the crate's reader: its header, its records, their
    /// subrecords.
    #[test]
    fn a_written_plugin_reads_back() {
        let perk = record(
            b"PERK",
            0x0000_0800,
            &[
                text(b"EDID", "NVRSTest"),
                text(b"FULL", "Test"),
                condition(495, 41, 0, Compare::GreaterOrEqual, 45.0),
                subrecord(b"DATA", &[0, 12, 1, 1, 0]),
            ],
        );
        let mut bytes = header(2, 0x801, "nv-rs", "test");
        bytes.extend_from_slice(&group(b"PERK", &[perk]));
        let plugin = Plugin::from_bytes(bytes).unwrap();
        let entry = plugin
            .records_of_type(FourCC(*b"PERK"))
            .next()
            .expect("the perk");
        assert_eq!(entry.header.form_id.0, 0x800);
        let ctda = plugin
            .subrecord_of(entry, FourCC(*b"CTDA"))
            .unwrap()
            .unwrap();
        assert_eq!(ctda.len(), 28);
        assert_eq!(ctda[0], 3 << 5);
        assert_eq!(u16::from_le_bytes([ctda[8], ctda[9]]), 495);
        let data = plugin.subrecord_of(entry, sig::DATA).unwrap().unwrap();
        assert_eq!(&data[..], &[0, 12, 1, 1, 0]);
        let full = plugin
            .subrecord_of(entry, FourCC(*b"FULL"))
            .unwrap()
            .unwrap();
        assert_eq!(&full[..], b"Test\0");
    }
}
