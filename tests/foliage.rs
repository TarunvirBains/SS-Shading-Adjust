//! Foliage warmth: only the foliage materials' primitive-color RGB bytes change, the combiner
//! (and with it the lighting term) is untouched, shared or non-multiply primitive colors are left
//! alone, and both cycle orders of `texture × shade × PRIM` are recognized.

use std::collections::HashSet;

use ss_shading_adjust::crc64::crc64;
use ss_shading_adjust::dl::{self, Cmd};
use ss_shading_adjust::foliage::{self, Foliage};

const LEAF: &str = "objects/test/gLeafTex";
const STONE: &str = "objects/test/gStoneTex";

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

/// (TEXEL0 * SHADE), then (COMBINED * PRIM): the bush and shrub order.
fn tex_shade_prim() -> Cmd {
    cmd(0xFC12_7E03, 0xFFFF_FDF8)
}

/// (TEXEL0 * PRIM), then (COMBINED * SHADE): the flower-leaf order.
fn tex_prim_shade() -> Cmd {
    cmd(0xFC11_9604, 0xFF5B_FFF8)
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

fn warm(strength: f32) -> Foliage {
    Foliage {
        enabled: true,
        tint: [1.0, 0.94, 0.62],
        strength,
        scenes: vec![],
    }
}

fn leaves() -> HashSet<u64> {
    [crc64(LEAF)].into()
}

#[test]
fn tint_scales_channels_keeps_alpha_and_never_lightens() {
    let f = warm(0.5);
    // White: R unchanged, G and B move halfway to the tint; alpha stays.
    let out = f.tint(0xFFFF_FFB3);
    assert_eq!(out & 0xFF, 0xB3);
    assert_eq!(out >> 24, 0xFF);
    assert_eq!((out >> 16) & 0xFF, 0xF7); // 255 * (0.5 + 0.5 * 0.94)
    assert_eq!((out >> 8) & 0xFF, 0xCF); // 255 * (0.5 + 0.5 * 0.62)
    // Never lighter than the old color in any channel.
    for old in [0x8040_20FFu32, 0x0000_00FF, 0xFFFF_FFFF, 0x9B8C_34B3] {
        let new = f.tint(old);
        for shift in [24, 16, 8] {
            assert!(((new >> shift) & 0xFF) <= ((old >> shift) & 0xFF));
        }
    }
    // Strength 0 changes nothing.
    assert_eq!(warm(0.0).tint(0xFFFF_FFFF), 0xFFFF_FFFF);
}

#[test]
fn only_the_leaf_prim_rgb_bytes_change() {
    let f = warm(0.5);
    let bytes = list(&[
        settimg(STONE),
        tex_shade_prim(),
        cmd(0xFA00_0000, 0x6090_30FF),
        tri(),
        settimg(LEAF),
        cmd(0xFA00_0000, 0xFFFF_FFFF),
        tri(),
    ]);
    let (out, changes) = foliage::patch_dl("x", &bytes, &leaves(), |v| f.tint(v)).unwrap();
    assert_eq!(changes.len(), 1, "{changes:?}");
    let c = &changes[0];
    assert!(c.skipped.is_empty(), "{c:?}");
    assert_ne!(c.to, c.from);
    assert_eq!(c.to & 0xFF, c.from & 0xFF, "alpha kept");
    let diff: Vec<usize> = (0..bytes.len()).filter(|&i| bytes[i] != out[i]).collect();
    assert!(
        !diff.is_empty()
            && diff
                .iter()
                .all(|&i| (c.offset + 4..c.offset + 8).contains(&i))
    );
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
    let f = warm(0.5);
    // One primitive color drawn by a leaf and a stone: skipped.
    let shared = list(&[
        tex_shade_prim(),
        cmd(0xFA00_0000, 0xFFFF_FFFF),
        settimg(STONE),
        tri(),
        settimg(LEAF),
        tri(),
    ]);
    let (out, changes) = foliage::patch_dl("x", &shared, &leaves(), |v| f.tint(v)).unwrap();
    assert_eq!(out, shared);
    assert!(changes.iter().all(|c| !c.skipped.is_empty()), "{changes:?}");
    // A leaf drawn with a combiner that does not multiply by PRIM: skipped.
    let blend = list(&[
        settimg(LEAF),
        detail_blend(),
        cmd(0xFA00_0000, 0xFFFF_FFFF),
        tri(),
    ]);
    let (out, changes) = foliage::patch_dl("x", &blend, &leaves(), |v| f.tint(v)).unwrap();
    assert_eq!(out, blend);
    assert!(changes.iter().all(|c| !c.skipped.is_empty()), "{changes:?}");
    // No leaf at all: nothing reported, nothing changed.
    let stone = list(&[
        settimg(STONE),
        tex_shade_prim(),
        cmd(0xFA00_0000, 0xFFFF_FFFF),
        tri(),
    ]);
    let (out, changes) = foliage::patch_dl("x", &stone, &leaves(), |v| f.tint(v)).unwrap();
    assert_eq!(out, stone);
    assert!(changes.is_empty());
}

#[test]
fn both_cycle_orders_are_recognized() {
    // Cycle 1 multiplies the texel by PRIM and cycle 2 the result by the shade.
    let f = warm(0.5);
    let bytes = list(&[
        settimg(LEAF),
        tex_prim_shade(),
        cmd(0xFA00_0000, 0xFFFF_FFFF),
        tri(),
    ]);
    let (_, a) = foliage::patch_dl("x", &bytes, &leaves(), |v| f.tint(v)).unwrap();
    assert_eq!(a.len(), 1);
}
