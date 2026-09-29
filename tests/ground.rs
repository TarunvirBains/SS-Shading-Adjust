//! Ground tint: only the earth materials' primitive-color RGB bytes change, their lightness is
//! kept, the combiner (and with it the lighting term) is untouched, shared or non-earth
//! primitive colors are left alone.

use std::collections::HashSet;

use ss_shading_adjust::crc64::crc64;
use ss_shading_adjust::dl::{self, Cmd};
use ss_shading_adjust::ground::{self, Ground, GroundScene};

const EARTH: &str = "scenes/shared/test_scene/test_room_0Tex_000000";
const GRASS: &str = "scenes/shared/test_scene/test_room_0Tex_001000";

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

/// Kokiri Forest's lit terrain combiner: (TEXEL0 * SHADE), then (COMBINED * PRIM).
fn tex_shade_prim() -> Cmd {
    cmd(0xFC12_7E03, 0xFFFF_FDF8)
}

/// A detail blend of two textures, then times the shade (no PRIM in the color path).
fn detail_blend() -> Cmd {
    cmd(0xFC26_7E04, 0x1FFC_FDF8)
}

fn tri() -> Cmd {
    cmd(0x0600_0204, 0x0000_0406)
}

fn list(cmds: &[Cmd]) -> Vec<u8> {
    let mut header = vec![0u8; dl::COMMANDS];
    header[4..8].copy_from_slice(b"TLDO");
    let mut all = cmds.to_vec();
    all.push(cmd(0xDF00_0000, 0));
    dl::encode(&header, &all)
}

fn config(strength: f32) -> (Ground, GroundScene) {
    let scene = GroundScene {
        name: "test".into(),
        enabled: true,
        dls: "**".into(),
        textures: vec![EARTH.into()],
        texture_lightness: 0.85,
    };
    (
        Ground {
            enabled: true,
            target_hue: 50.0,
            target_chroma: 0.085,
            strength,
            scenes: vec![scene.clone()],
        },
        scene,
    )
}

fn lab(rgba: u32) -> [f32; 3] {
    let c = |s: u32| ((rgba >> s) & 0xFF) as f32 / 255.0;
    ground::srgb_to_oklab([c(24), c(16), c(8)])
}

fn earth_set() -> HashSet<u64> {
    [crc64(EARTH)].into()
}

#[test]
fn only_the_earth_prim_rgb_bytes_change_and_lightness_is_kept() {
    let (g, scene) = config(0.85);
    let bytes = list(&[
        settimg(GRASS),
        tex_shade_prim(),
        cmd(0xFA00_0000, 0x6090_30FF),
        tri(),
        settimg(EARTH),
        cmd(0xFA00_0000, 0x9B8C_34B3),
        tri(),
    ]);
    let (out, changes) =
        ground::patch_dl("x", &bytes, &earth_set(), |v| g.tint(&scene, v)).unwrap();
    assert_eq!(changes.len(), 1, "{changes:?}");
    let c = &changes[0];
    assert!(c.skipped.is_empty());
    assert_eq!(c.to & 0xFF, c.from & 0xFF, "alpha kept");
    // Only the RGB bytes of that one command differ.
    let diff: Vec<usize> = (0..bytes.len()).filter(|&i| bytes[i] != out[i]).collect();
    assert!(
        !diff.is_empty()
            && diff
                .iter()
                .all(|&i| (c.offset + 5..c.offset + 8).contains(&i))
    );
    // Lightness kept (within a byte's rounding), hue moved toward the target.
    let (a, b) = (lab(c.from), lab(c.to));
    assert!((a[0] - b[0]).abs() < 0.01, "L {} -> {}", a[0], b[0]);
    let hue = |l: [f32; 3]| l[2].atan2(l[1]).to_degrees().rem_euclid(360.0);
    assert!(hue(b) < hue(a) && (hue(b) - 50.0).abs() < (hue(a) - 50.0).abs());
    // The combiner (with its lighting term) is untouched, and the list still parses the same.
    let (p, q) = (dl::parse(&bytes).unwrap(), dl::parse(&out).unwrap());
    assert_eq!(p.len(), q.len());
    for (x, y) in p.iter().zip(&q) {
        if x.op() != dl::G_SETPRIMCOLOR {
            assert_eq!(x, y);
        }
    }
    assert_eq!(dl::encode(&out, &q), out, "round trip");
}

#[test]
fn shared_and_non_multiply_prims_are_skipped() {
    let (g, scene) = config(0.85);
    // One primitive color drawn by grass and earth: skipped.
    let shared = list(&[
        tex_shade_prim(),
        cmd(0xFA00_0000, 0x9B8C_34FF),
        settimg(GRASS),
        tri(),
        settimg(EARTH),
        tri(),
    ]);
    let (out, changes) =
        ground::patch_dl("x", &shared, &earth_set(), |v| g.tint(&scene, v)).unwrap();
    assert_eq!(out, shared);
    assert!(changes.iter().all(|c| !c.skipped.is_empty()), "{changes:?}");
    // Earth drawn with a combiner that does not multiply by PRIM: skipped.
    let blend = list(&[
        settimg(EARTH),
        detail_blend(),
        cmd(0xFA00_0000, 0xFFFF_FFFF),
        tri(),
    ]);
    let (out, changes) =
        ground::patch_dl("x", &blend, &earth_set(), |v| g.tint(&scene, v)).unwrap();
    assert_eq!(out, blend);
    assert_eq!(changes.len(), 1);
    assert!(!changes[0].skipped.is_empty());
}

#[test]
fn strength_zero_changes_nothing_and_strength_scales_the_move() {
    let prim = 0x9B8C_34B3;
    let hue = |v: u32| {
        let l = lab(v);
        l[2].atan2(l[1]).to_degrees().rem_euclid(360.0)
    };
    let (g0, s0) = config(0.0);
    assert_eq!(g0.tint(&s0, prim), prim);
    let (ga, sa) = config(0.5);
    let (gb, sb) = config(0.85);
    let (a, b) = (ga.tint(&sa, prim), gb.tint(&sb, prim));
    assert!(
        hue(prim) > hue(a) && hue(a) > hue(b),
        "{} {} {}",
        hue(prim),
        hue(a),
        hue(b)
    );
}
