//! Patching changes only alpha bytes, the mapping keeps order and ratios and never exceeds
//! `max_alpha`, and display lists round-trip. Synthetic display lists; set `OOT_O2R` to a game
//! archive to also round-trip every display list in it.

use std::collections::HashSet;

use oot_deeper_water::config::{Config, Mapping, Surface};
use oot_deeper_water::crc64::crc64;
use oot_deeper_water::dl::{self, Cmd};
use oot_deeper_water::water::patch_dl;

const WATER: &str = "scenes/shared/test_scene/test_room_0Tex_000000";
const ROCK: &str = "scenes/shared/test_scene/test_room_0Tex_001000";

fn cmd(w0: u32, w1: u32) -> Cmd {
    Cmd {
        offset: 0,
        w0,
        w1,
        extra: None,
    }
}

fn settimg(path: &str) -> Cmd {
    Cmd {
        extra: Some(crc64(path)),
        ..cmd(0x2050_0000, 0)
    }
}

fn prim(alpha: u8) -> Cmd {
    cmd(0xFA00_0000, 0xFFFF_FF00 | alpha as u32)
}

/// The Kokiri Forest pond's combiner: output alpha = primitive alpha (two-cycle).
fn combine_prim() -> Cmd {
    cmd(0xFC26_7E04, 0x1F0C_FDFF)
}

/// A combiner whose output alpha is 1 (the primitive alpha is not used).
fn combine_opaque() -> Cmd {
    cmd(0xFC12_7E03, 0xFFFF_F3F8)
}

/// A display list: a translucent water material, a translucent rock material, an opaque-combiner
/// water material, and triangles between them.
fn display_list() -> Vec<u8> {
    let mut header = vec![0u8; dl::COMMANDS];
    header[4..8].copy_from_slice(b"TLDO");
    header[0x40] = 4;
    let tri = cmd(0x0500_0204, 0);
    let cmds = [
        Cmd {
            extra: Some(0x0FB9_18B0_F5C1_F722),
            ..cmd(0x3300_0000, 0xBEEF_BEEF)
        },
        cmd(0xE300_1001, 0),
        settimg(WATER),
        combine_prim(),
        prim(99),
        tri,
        settimg(ROCK),
        prim(128),
        tri,
        settimg(WATER),
        combine_opaque(),
        prim(64),
        tri,
        cmd(0xDF00_0000, 0),
    ];
    dl::encode(&header, &cmds)
}

fn config(max_alpha: f32) -> Config {
    Config {
        max_alpha,
        mapping: Mapping::Proportional,
        a_max: None,
        water_textures: vec![WATER.into()],
        surfaces: Vec::new(),
    }
}

fn surface(max_alpha: Option<f32>) -> Surface {
    Surface {
        dl: "x".into(),
        max_alpha,
        reason: String::new(),
    }
}

#[test]
fn display_lists_round_trip() {
    let bytes = display_list();
    let cmds = dl::parse(&bytes).unwrap();
    assert_eq!(dl::encode(&bytes, &cmds), bytes);
    // Truncated or unterminated lists are rejected.
    assert!(dl::parse(&bytes[..bytes.len() - 8]).is_err());
}

#[test]
fn only_the_water_materials_alpha_bytes_change() {
    let bytes = display_list();
    let water: HashSet<u64> = [crc64(WATER)].into();
    let (out, changes) = patch_dl("x", &bytes, &water, |a| a.saturating_add(50)).unwrap();
    // Only the first material: water, and its combiner uses the primitive alpha.
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!((changes[0].from, changes[0].to), (99, 149));
    let diff: Vec<usize> = (0..bytes.len()).filter(|&i| bytes[i] != out[i]).collect();
    assert_eq!(diff, vec![changes[0].offset + 4]);
    assert_eq!(out.len(), bytes.len());
    assert_eq!(
        dl::parse(&out).unwrap().len(),
        dl::parse(&bytes).unwrap().len()
    );
}

#[test]
fn the_mapping_lifts_every_surface_keeps_ratios_and_order_and_never_exceeds_max_alpha() {
    let c = config(0.70);
    let a_max = 0.50;
    let mut prev = 0.0;
    for i in 1..=255u8 {
        let a = i as f32 / 255.0;
        let m = c.mapped(a, a_max);
        assert!(m >= prev, "order lost at {a}");
        prev = m;
        if !c.at_or_above_max(a) {
            assert!(m > a, "{a} not lifted");
            assert!(m <= c.max_alpha + 1e-6, "{a} -> {m}");
        } else {
            assert_eq!(m, a, "water at or above max_alpha keeps its value");
        }
    }
    // Ratios: 0.39 / 0.45 is kept, and a_max lands on max_alpha (within half a percent).
    let (p, q, d) = (
        c.mapped(0.39, a_max),
        c.mapped(0.45, a_max),
        c.mapped(0.50, a_max),
    );
    assert!(
        (p / q - 0.39 / 0.45).abs() < 1e-5 && (d - 0.70).abs() <= 0.005 + 1e-6,
        "{p} {d}"
    );
    assert!((p - 0.546).abs() < 1e-3, "Kokiri pond {p}");
}

#[test]
fn gameplay_limits_cap_a_surface_and_nothing_goes_down() {
    let c = config(0.70);
    let limited = surface(Some(0.55));
    for i in 1..=255u8 {
        let t = c.target(&limited, i, 0.5);
        assert!(t >= i, "lowered {i} -> {t}");
        if i as f32 / 255.0 <= 0.55 {
            assert!(t as f32 / 255.0 <= 0.55 + 0.5 / 255.0, "{i} -> {t}");
        }
        let free = c.target(&surface(None), i, 0.5);
        assert!(free >= t, "a limit may only lower the result");
        if (i as f32) / 255.0 < 0.70 {
            assert!(free as f32 / 255.0 <= 0.70 + 0.5 / 255.0, "{i} -> {free}");
        }
    }
}

#[test]
fn configs_reject_limits_above_max_alpha() {
    let mut c = config(0.70);
    c.surfaces.push(Surface {
        max_alpha: Some(0.8),
        ..surface(None)
    });
    assert!(c.validate().is_err());
    let c: Result<Config, _> =
        toml::from_str("max_alpha = 0.7\nmapping = 'linear'\nwater_textures = []");
    assert!(c.is_err());
}

#[test]
fn every_display_list_of_a_game_archive_round_trips() {
    let Ok(path) = std::env::var("OOT_O2R") else {
        eprintln!("OOT_O2R not set: skipped");
        return;
    };
    let mut archive = oot_deeper_water::o2r::Archive::open(std::path::Path::new(&path)).unwrap();
    let mut n = 0;
    for name in archive.names() {
        let bytes = archive.read(&name).unwrap();
        if !dl::is_dl(&bytes) {
            continue;
        }
        let cmds = dl::parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(dl::encode(&bytes, &cmds), bytes, "{name}");
        n += 1;
    }
    assert!(n > 1000, "{n} display lists");
}

#[test]
fn water_at_or_above_max_alpha_is_never_changed() {
    let c = config(0.70);
    for byte in [178u8, 179, 204, 230] {
        let a = byte as f32 / 255.0;
        assert!(c.at_or_above_max(a), "{a}");
        assert_eq!(c.mapped(a, 0.5), a);
        assert_eq!(c.target(&surface(None), byte, 0.5), byte);
    }
    assert!(!c.at_or_above_max(166.0 / 255.0));
}
