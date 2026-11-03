//! Scene lighting warmth: daytime entries get warmer ambient, light and fog colors; night
//! entries, directions, fog distances and every other byte stay; resources without exactly one
//! parseable lighting list are left alone.

use ss_shading_adjust::lighting::{self, ENTRY, Lighting};

fn cfg(strength: f32) -> Lighting {
    Lighting {
        enabled: true,
        day_min_luma: 0.28,
        ambient_tint: [1.0, 0.95, 0.80],
        light_tint: [1.0, 0.96, 0.82],
        fog_tint: [1.0, 0.97, 0.88],
        strength,
        scenes: vec![],
    }
}

/// Noon: ambient 50 50 50, sun white, fog c8 c8 96, fog far 5800.
fn noon() -> [u8; ENTRY] {
    [
        0x50, 0x50, 0x50, 0x49, 0x49, 0x49, 0xFF, 0xFF, 0xFF, 0xB7, 0xB7, 0xB7, 0x46, 0x46, 0x5A,
        0xC8, 0xC8, 0x96, 0xE2, 0xFF, 0xA8, 0x16,
    ]
}

/// Night: ambient 14 14 3c, fog 14 14 3c, fog far 5800.
fn night() -> [u8; ENTRY] {
    [
        0x14, 0x14, 0x3C, 0x49, 0x49, 0x49, 0x32, 0x32, 0x5A, 0xB7, 0xB7, 0xB7, 0x1E, 0x1E, 0x3C,
        0x14, 0x14, 0x3C, 0xBC, 0x07, 0xA8, 0x16,
    ]
}

/// A scene resource: a MORO header, some other command bytes, the lighting list, a tail.
fn resource(entries: &[[u8; ENTRY]]) -> Vec<u8> {
    let mut b = vec![0, 0, 0, 0];
    b.extend_from_slice(b"MORO");
    b.extend_from_slice(&[0xEE; 40]);
    b.extend_from_slice(&[0x0F, 0, 0, 0, entries.len() as u8, 0, 0, 0]);
    for e in entries {
        b.extend_from_slice(e);
    }
    b.extend_from_slice(&[0x11, 0, 0, 0, 0x22, 0x33]);
    b
}

#[test]
fn only_day_entries_change_and_only_color_bytes() {
    let c = cfg(0.5);
    let bytes = resource(&[noon(), night(), noon()]);
    let (out, changes) = lighting::patch_resource("scene", &bytes, &c).unwrap();
    assert_eq!(changes.len(), 3);
    assert!(changes[0].kept.is_empty() && !changes[1].kept.is_empty());
    assert_eq!(out.len(), bytes.len());
    let (start, _) = lighting::find_list(&bytes).unwrap();
    // The night entry and every byte outside the color bytes of day entries are unchanged.
    let color = |i: usize| (0..3).contains(&i) || (6..9).contains(&i) || (12..18).contains(&i);
    for (i, (a, b)) in bytes.iter().zip(&out).enumerate() {
        if a != b {
            let rel = i - start;
            assert!(rel < 3 * ENTRY, "outside the list: byte {i}");
            assert_ne!(rel / ENTRY, 1, "night entry changed");
            assert!(color(rel % ENTRY), "non-color byte {rel} changed");
            assert!(b <= a, "byte {rel} got lighter: {a} -> {b}");
        }
    }
    // Noon's ambient 50 50 50 warms toward the tint: blue drops most, red stays.
    assert_eq!(&out[start..start + 3], &[0x50, 0x4E, 0x48]);
}

#[test]
fn zero_strength_changes_nothing() {
    let bytes = resource(&[noon(), night()]);
    let (out, _) = lighting::patch_resource("scene", &bytes, &cfg(0.0)).unwrap();
    assert_eq!(out, bytes);
}

#[test]
fn unparseable_resources_are_left_alone() {
    let c = cfg(0.5);
    // No MORO header.
    let mut raw = resource(&[noon()]);
    raw[4..8].copy_from_slice(b"XXXX");
    let (out, changes) = lighting::patch_resource("s", &raw, &c).unwrap();
    assert_eq!(out, raw);
    assert!(changes.is_empty());
    // Implausible fog far (0): not a lighting list.
    let mut bad = noon();
    bad[20] = 0;
    bad[21] = 0;
    let bytes = resource(&[bad]);
    let (out, changes) = lighting::patch_resource("s", &bytes, &c).unwrap();
    assert_eq!(out, bytes);
    assert!(changes.is_empty());
    // Two candidate lists: ambiguous, left alone.
    let mut two = resource(&[noon()]);
    two.extend_from_slice(&[0x0F, 0, 0, 0, 1, 0, 0, 0]);
    two.extend_from_slice(&noon());
    let (out, changes) = lighting::patch_resource("s", &two, &c).unwrap();
    assert_eq!(out, two);
    assert!(changes.is_empty());
    // Cutscene data is never touched.
    let bytes = resource(&[noon()]);
    let (out, changes) =
        lighting::patch_resource("x_sceneSet_00CutsceneData_01", &bytes, &c).unwrap();
    assert_eq!(out, bytes);
    assert!(changes.is_empty());
}
