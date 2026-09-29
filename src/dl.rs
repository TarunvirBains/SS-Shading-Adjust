//! libultraship display-list resources (`DisplayList`, type tag `TLDO` at 0x04): a 0x40-byte resource header,
//! the microcode byte padded to 8, then F3DEX2 commands of 8 bytes (two little-endian words),
//! some followed by an extra 8-byte word (resource hashes). The last command is `G_ENDDL`.

use anyhow::{Result, bail, ensure};

/// Where the commands start.
pub const COMMANDS: usize = 0x48;

pub const G_VTX: u8 = 0x01;
pub const G_TRI1: u8 = 0x05;
pub const G_TRI2: u8 = 0x06;
pub const G_QUAD: u8 = 0x07;
pub const G_SETTIMG_OTR_HASH: u8 = 0x20;
pub const G_DL_OTR_HASH: u8 = 0x31;
pub const G_VTX_OTR_HASH: u8 = 0x32;
pub const G_MARKER: u8 = 0x33;
pub const G_BRANCH_Z_OTR: u8 = 0x35;
pub const G_MTX_OTR: u8 = 0x36;
pub const G_DL: u8 = 0xDE;
pub const G_ENDDL: u8 = 0xDF;
pub const G_SETOTHERMODE_H: u8 = 0xE3;
pub const G_SETOTHERMODE_L: u8 = 0xE2;
pub const G_SETPRIMCOLOR: u8 = 0xFA;
pub const G_SETENVCOLOR: u8 = 0xFB;
pub const G_SETCOMBINE: u8 = 0xFC;

/// Commands followed by an extra 8-byte word.
fn has_extra(op: u8) -> bool {
    matches!(
        op,
        G_SETTIMG_OTR_HASH | G_DL_OTR_HASH | G_VTX_OTR_HASH | G_MARKER | G_BRANCH_Z_OTR | G_MTX_OTR
    )
}

/// One command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cmd {
    /// Byte offset in the resource.
    pub offset: usize,
    pub w0: u32,
    pub w1: u32,
    /// The extra word (a resource hash, as libultraship reads it).
    pub extra: Option<u64>,
}

impl Cmd {
    pub fn op(&self) -> u8 {
        (self.w0 >> 24) as u8
    }

    /// Encoded size in bytes.
    pub fn len(&self) -> usize {
        if self.extra.is_some() { 16 } else { 8 }
    }

    pub fn is_empty(&self) -> bool {
        false
    }
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// True if the bytes are a display-list resource.
pub fn is_dl(bytes: &[u8]) -> bool {
    bytes.len() >= COMMANDS + 8 && &bytes[4..8] == b"TLDO"
}

/// A resource hash stored after a command: the high word first, each little-endian.
fn hash_at(b: &[u8], o: usize) -> u64 {
    ((u32_at(b, o) as u64) << 32) | u32_at(b, o + 4) as u64
}

/// Parses every command. The list must end with `G_ENDDL` exactly at the end of the resource.
pub fn parse(bytes: &[u8]) -> Result<Vec<Cmd>> {
    ensure!(is_dl(bytes), "not a display-list resource");
    let mut cmds = Vec::new();
    let mut o = COMMANDS;
    while o < bytes.len() {
        ensure!(o + 8 <= bytes.len(), "truncated command at {o:#x}");
        let (w0, w1) = (u32_at(bytes, o), u32_at(bytes, o + 4));
        let op = (w0 >> 24) as u8;
        let extra = if has_extra(op) {
            ensure!(o + 16 <= bytes.len(), "truncated extra word at {o:#x}");
            Some(hash_at(bytes, o + 8))
        } else {
            None
        };
        let cmd = Cmd {
            offset: o,
            w0,
            w1,
            extra,
        };
        o += cmd.len();
        cmds.push(cmd);
        if op == G_ENDDL {
            if o != bytes.len() {
                bail!(
                    "G_ENDDL at {:#x} before the end ({:#x})",
                    cmd.offset,
                    bytes.len()
                );
            }
            return Ok(cmds);
        }
    }
    bail!("no G_ENDDL")
}

/// Encodes commands after `header` (the first [`COMMANDS`] bytes of the resource).
pub fn encode(header: &[u8], cmds: &[Cmd]) -> Vec<u8> {
    let mut out = header[..COMMANDS].to_vec();
    for c in cmds {
        out.extend(c.w0.to_le_bytes());
        out.extend(c.w1.to_le_bytes());
        if let Some(h) = c.extra {
            out.extend(((h >> 32) as u32).to_le_bytes());
            out.extend((h as u32).to_le_bytes());
        }
    }
    out
}

/// A combiner input that can feed the final alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Texel0,
    Texel1,
    Prim,
    Shade,
    Env,
    LodFrac,
}

/// The inputs the final (output) alpha of a combiner depends on. `two_cycle`: the second
/// cycle's alpha is the output and may use the first's (`COMBINED`).
pub fn alpha_sources(w0: u32, w1: u32, two_cycle: bool) -> Vec<Source> {
    let f = |w: u32, hi: u32, bits: u32| (w >> hi) & ((1 << bits) - 1);
    // (a, b, c, d) per cycle.
    let c0 = [f(w0, 12, 3), f(w1, 12, 3), f(w0, 9, 3), f(w1, 9, 3)];
    let c1 = [f(w1, 21, 3), f(w1, 3, 3), f(w1, 18, 3), f(w1, 0, 3)];
    // a, b, d: 0 COMBINED, 1 TEXEL0, 2 TEXEL1, 3 PRIM, 4 SHADE, 5 ENV, 6 ONE, 7 ZERO.
    // c: 0 LOD_FRACTION, 1 TEXEL0, 2 TEXEL1, 3 PRIM, 4 SHADE, 5 ENV, 6 PRIM_LOD_FRAC, 7 ZERO.
    fn abd(v: u32, combined: &[Source]) -> Vec<Source> {
        match v {
            0 => combined.to_vec(),
            1 => vec![Source::Texel0],
            2 => vec![Source::Texel1],
            3 => vec![Source::Prim],
            4 => vec![Source::Shade],
            5 => vec![Source::Env],
            _ => vec![],
        }
    }
    fn cc(v: u32) -> Vec<Source> {
        match v {
            0 => vec![Source::LodFrac],
            1 => vec![Source::Texel0],
            2 => vec![Source::Texel1],
            3 => vec![Source::Prim],
            4 => vec![Source::Shade],
            5 => vec![Source::Env],
            6 => vec![Source::Prim],
            _ => vec![],
        }
    }
    let eval = |[a, b, c, d]: [u32; 4], combined: &[Source]| -> Vec<Source> {
        let mut s = abd(d, combined);
        // (A - B) * C vanishes when C is zero or A equals B.
        if c != 7 && a != b {
            s.extend(abd(a, combined));
            s.extend(abd(b, combined));
            s.extend(cc(c));
        }
        s.sort();
        s.dedup();
        s
    };
    let first = eval(c0, &[]);
    if two_cycle { eval(c1, &first) } else { first }
}

/// A primitive color whose alpha feeds the output of the material it is set for.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimAlpha {
    /// Offset of the `G_SETPRIMCOLOR` command.
    pub offset: usize,
    pub alpha: u8,
    /// Hashes of the textures the material draws with.
    pub textures: Vec<u64>,
    /// Whether the combiner's output alpha depends on the primitive alpha.
    pub used: bool,
    /// The combiner words in effect.
    pub combine: Option<(u32, u32)>,
}

/// The primitive colors set in a display list, each resolved against the material it applies to:
/// the textures and combiner in effect at the next draw (or at the end of the list, for material
/// lists called before the geometry).
pub fn prim_alphas(cmds: &[Cmd]) -> Vec<PrimAlpha> {
    let mut out = Vec::new();
    let mut pending: Vec<(usize, u8)> = Vec::new();
    let mut textures: Vec<u64> = Vec::new();
    let mut drew = true;
    let mut combine = None;
    // G_MDSFT_CYCLETYPE = 20, 2 bits: 0 one cycle, 1 two cycles; unknown until set.
    let mut two_cycle = true;
    let mut resolve = |pending: &mut Vec<(usize, u8)>,
                       textures: &[u64],
                       combine: Option<(u32, u32)>,
                       two_cycle: bool| {
        for (offset, alpha) in pending.drain(..) {
            let used = combine
                .is_some_and(|(w0, w1)| alpha_sources(w0, w1, two_cycle).contains(&Source::Prim));
            out.push(PrimAlpha {
                offset,
                alpha,
                textures: textures.to_vec(),
                used,
                combine,
            });
        }
    };
    for c in cmds {
        match c.op() {
            G_SETTIMG_OTR_HASH => {
                if drew {
                    textures.clear();
                    drew = false;
                }
                textures.push(c.extra.unwrap_or(0));
            }
            G_SETCOMBINE => combine = Some((c.w0 & 0x00FF_FFFF, c.w1)),
            G_SETOTHERMODE_H => {
                let shift = 32 - ((c.w0 >> 8) & 0xFF) - (c.w0 & 0xFF) - 1;
                let len = (c.w0 & 0xFF) + 1;
                if shift <= 20 && 20 + 2 <= shift + len {
                    two_cycle = (c.w1 >> 20) & 3 == 1;
                }
            }
            G_SETPRIMCOLOR => pending.push((c.offset, c.w1 as u8)),
            G_VTX | G_TRI1 | G_TRI2 | G_QUAD | G_VTX_OTR_HASH | G_DL | G_DL_OTR_HASH | G_ENDDL => {
                if matches!(
                    c.op(),
                    G_TRI1 | G_TRI2 | G_QUAD | G_DL | G_DL_OTR_HASH | G_ENDDL
                ) {
                    resolve(&mut pending, &textures, combine, two_cycle);
                }
                drew = true;
            }
            _ => {}
        }
    }
    out
}

/// Sets the alpha byte of the `G_SETPRIMCOLOR` command at `offset` (the low byte of the second
/// word). Nothing else changes.
pub fn set_prim_alpha(bytes: &mut [u8], offset: usize, alpha: u8) -> Result<()> {
    ensure!(
        offset + 8 <= bytes.len() && bytes[offset + 3] == G_SETPRIMCOLOR,
        "no G_SETPRIMCOLOR at {offset:#x}"
    );
    bytes[offset + 4] = alpha;
    Ok(())
}
