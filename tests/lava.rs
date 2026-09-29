//! Lava glow: only the lava materials' combiner (cycle 2 gains PRIM as an additive term; the
//! lighting term stays) and primitive-color RGB change; shared commands are left alone.

use std::collections::HashSet;

use ss_shading_adjust::crc64::crc64;
use ss_shading_adjust::dl::{self, Cmd};
use ss_shading_adjust::lava::{self, Form};
use ss_shading_adjust::material::{self, In};

const LAVA: &str = "scenes/nonmq/test_scene/test_room_0Tex_000000";
const ROCK: &str = "scenes/nonmq/test_scene/test_room_0Tex_001000";

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

/// The Fire Temple lava: detail blend of two textures, then times the shade.
fn blend_times_shade() -> Cmd {
    cmd(0xFC26_7E04, 0x1FFC_FDF8)
}

/// Dodongo's Cavern lava: (TEXEL0 * SHADE), then (COMBINED * PRIM).
fn tex_shade_prim() -> Cmd {
    cmd(0xFC12_7E03, 0xFFFF_FDF8)
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

fn lava_set() -> HashSet<u64> {
    [crc64(LAVA)].into()
}

const GLOW: u32 = 0x362B_04FF;

fn changed_bytes(a: &[u8], b: &[u8]) -> Vec<usize> {
    (0..a.len()).filter(|&i| a[i] != b[i]).collect()
}

#[test]
fn the_shaded_form_gains_prim_as_an_additive_term() {
    let bytes = list(&[
        settimg(LAVA),
        blend_times_shade(),
        cmd(0xFA00_0000, 0xFFFF_FF80),
        tri(),
    ]);
    let (out, changes) = lava::patch_dl("x", &bytes, &lava_set(), GLOW).unwrap();
    assert_eq!(changes.len(), 1);
    assert!(changes[0].skipped.is_empty(), "{changes:?}");
    assert_eq!(changes[0].form, Some(Form::TimesX));
    let cmds = dl::parse(&out).unwrap();
    let comb = cmds.iter().find(|c| c.op() == dl::G_SETCOMBINE).unwrap();
    let [x, y] = material::color_cycles(comb.w0 & 0xFF_FFFF, comb.w1);
    let [x0, _] = material::color_cycles(0x26_7E04, 0x1FFC_FDF8);
    assert_eq!(x, x0, "cycle 1 unchanged");
    assert_eq!(
        (y.a, y.b, y.c, y.d),
        (In::Combined, In::Zero, In::Shade, In::Prim)
    );
    // The alpha path is untouched (the alpha bits of both words).
    assert_eq!(comb.w0 & 0x7E00, 0x7E04 & 0x7E00);
    assert_eq!(comb.w1 & !0xFF03_81C0, 0x1FFC_FDF8 & !0xFF03_81C0);
    let prim = cmds.iter().find(|c| c.op() == dl::G_SETPRIMCOLOR).unwrap();
    assert_eq!(prim.w1, (GLOW & 0xFFFF_FF00) | 0x80, "glow RGB, alpha kept");
    // Only the combiner and primitive-color commands changed; the list round-trips.
    let offs: Vec<usize> = changed_bytes(&bytes, &out);
    assert!(offs.iter().all(|&i| {
        (changes[0].combine..changes[0].combine + 8).contains(&i)
            || (changes[0].prim + 5..changes[0].prim + 8).contains(&i)
    }));
    assert_eq!(dl::encode(&out, &cmds), out);
}

#[test]
fn the_prim_multiply_form_becomes_an_additive_prim() {
    let bytes = list(&[
        settimg(LAVA),
        tex_shade_prim(),
        cmd(0xFA00_0000, 0xFFFF_FFFF),
        tri(),
    ]);
    let (out, changes) = lava::patch_dl("x", &bytes, &lava_set(), GLOW).unwrap();
    assert!(changes[0].skipped.is_empty(), "{changes:?}");
    let cmds = dl::parse(&out).unwrap();
    let comb = cmds.iter().find(|c| c.op() == dl::G_SETCOMBINE).unwrap();
    let [x, y] = material::color_cycles(comb.w0 & 0xFF_FFFF, comb.w1);
    assert_eq!(
        (x.a, x.c),
        (In::Texel0, In::Shade),
        "the lighting term stays"
    );
    assert_eq!(
        (y.a, y.b, y.c, y.d),
        (In::Prim, In::Zero, In::Other(10), In::Combined)
    );
}

#[test]
fn shared_commands_and_rock_are_left_alone() {
    // One combiner and primitive color drawn by lava and rock: skipped.
    let bytes = list(&[
        blend_times_shade(),
        cmd(0xFA00_0000, 0xFFFF_FFFF),
        settimg(ROCK),
        tri(),
        settimg(LAVA),
        tri(),
    ]);
    let (out, changes) = lava::patch_dl("x", &bytes, &lava_set(), GLOW).unwrap();
    assert_eq!(out, bytes);
    assert!(changes.iter().all(|c| !c.skipped.is_empty()));
    // Rock only: nothing to do.
    let rock = list(&[
        settimg(ROCK),
        blend_times_shade(),
        cmd(0xFA00_0000, 0xFFFF_FFFF),
        tri(),
    ]);
    let (out, changes) = lava::patch_dl("x", &rock, &lava_set(), GLOW).unwrap();
    assert_eq!(out, rock);
    assert!(changes.is_empty());
}
