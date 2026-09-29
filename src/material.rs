//! Materials of a display list: the render state (textures, combiner, primitive and environment
//! colors, cycle type, lighting) in effect for each run of triangles, with the offsets of the
//! commands that set it.

use crate::dl::{self, Cmd};

/// A combiner input of the color path (`None` for the constants 0 and 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum In {
    Combined,
    Texel0,
    Texel1,
    Prim,
    Shade,
    Env,
    One,
    Zero,
    /// Anything else (alpha channels as color, noise, LOD, keys).
    Other(u8),
}

/// One cycle of the color combiner: `(a - b) * c + d`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cycle {
    pub a: In,
    pub b: In,
    pub c: In,
    pub d: In,
}

fn in_ab(v: u32) -> In {
    match v {
        0 => In::Combined,
        1 => In::Texel0,
        2 => In::Texel1,
        3 => In::Prim,
        4 => In::Shade,
        5 => In::Env,
        6 => In::One,
        15 => In::Zero,
        v => In::Other(v as u8),
    }
}

fn in_c(v: u32) -> In {
    match v {
        0 => In::Combined,
        1 => In::Texel0,
        2 => In::Texel1,
        3 => In::Prim,
        4 => In::Shade,
        5 => In::Env,
        31 => In::Zero,
        v => In::Other(v as u8),
    }
}

fn in_d(v: u32) -> In {
    match v {
        0 => In::Combined,
        1 => In::Texel0,
        2 => In::Texel1,
        3 => In::Prim,
        4 => In::Shade,
        5 => In::Env,
        6 => In::One,
        7 => In::Zero,
        v => In::Other(v as u8),
    }
}

/// The two color cycles of a `G_SETCOMBINE` (low 24 bits of w0, and w1).
pub fn color_cycles(w0: u32, w1: u32) -> [Cycle; 2] {
    let f = |w: u32, lo: u32, bits: u32| (w >> lo) & ((1 << bits) - 1);
    [
        Cycle {
            a: in_ab(f(w0, 20, 4)),
            b: in_ab(f(w1, 28, 4)),
            c: in_c(f(w0, 15, 5)),
            d: in_d(f(w1, 15, 3)),
        },
        Cycle {
            a: in_ab(f(w0, 5, 4)),
            b: in_ab(f(w1, 24, 4)),
            c: in_c(f(w0, 0, 5)),
            d: in_d(f(w1, 6, 3)),
        },
    ]
}

/// Render state for a run of triangles.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    /// Texture hashes loaded for it.
    pub textures: Vec<u64>,
    /// Offset and words of the `G_SETCOMBINE` in effect (`None` if set outside the list).
    pub combine: Option<(usize, u32, u32)>,
    /// Offset and RGBA of the `G_SETPRIMCOLOR` in effect.
    pub prim: Option<(usize, u32)>,
    pub env: Option<(usize, u32)>,
    /// Two-cycle mode (`None`: set outside the list).
    pub two_cycle: Option<bool>,
    /// Lighting on (vertex colors are normals); `None`: set outside the list.
    pub lit: Option<bool>,
    /// Triangles drawn with it.
    pub tris: usize,
    /// Offsets of the vertex loads drawn with it (`G_VTX_OTR_HASH`), with their array hashes.
    pub vtx: Vec<(usize, u64)>,
}

/// The materials of a display list, one per run of triangles drawn with an unchanged state.
pub fn materials(cmds: &[Cmd]) -> Vec<Material> {
    let mut out: Vec<Material> = Vec::new();
    let mut textures: Vec<u64> = Vec::new();
    let mut drew = true;
    let mut cur = Material {
        textures: Vec::new(),
        combine: None,
        prim: None,
        env: None,
        two_cycle: None,
        lit: None,
        tris: 0,
        vtx: Vec::new(),
    };
    let mut vtx: Vec<(usize, u64)> = Vec::new();
    for c in cmds {
        match c.op() {
            dl::G_SETTIMG_OTR_HASH => {
                if drew {
                    textures.clear();
                    drew = false;
                }
                textures.push(c.extra.unwrap_or(0));
            }
            dl::G_SETCOMBINE => cur.combine = Some((c.offset, c.w0 & 0x00FF_FFFF, c.w1)),
            dl::G_SETPRIMCOLOR => cur.prim = Some((c.offset, c.w1)),
            dl::G_SETENVCOLOR => cur.env = Some((c.offset, c.w1)),
            dl::G_SETOTHERMODE_H => {
                let len = (c.w0 & 0xFF) + 1;
                let shift = 32 - ((c.w0 >> 8) & 0xFF) - len;
                if shift <= 20 && 22 <= shift + len {
                    cur.two_cycle = Some((c.w1 >> 20) & 3 == 1);
                }
            }
            0xD9 => {
                let clear = !(c.w0 | 0xFF00_0000);
                if clear & 0x0002_0000 != 0 {
                    cur.lit = Some(false);
                }
                if c.w1 & 0x0002_0000 != 0 {
                    cur.lit = Some(true);
                }
            }
            dl::G_VTX_OTR_HASH => vtx.push((c.offset, c.extra.unwrap_or(0))),
            dl::G_TRI1 | dl::G_TRI2 | dl::G_QUAD => {
                drew = true;
                let n = if c.op() == dl::G_TRI1 { 1 } else { 2 };
                let same = out.last().is_some_and(|m| {
                    m.textures == textures
                        && m.combine == cur.combine
                        && m.prim == cur.prim
                        && m.env == cur.env
                        && m.two_cycle == cur.two_cycle
                        && m.lit == cur.lit
                });
                if same {
                    let m = out.last_mut().unwrap();
                    m.tris += n;
                    for v in vtx.drain(..) {
                        if !m.vtx.contains(&v) {
                            m.vtx.push(v);
                        }
                    }
                } else {
                    out.push(Material {
                        textures: textures.clone(),
                        tris: n,
                        vtx: std::mem::take(&mut vtx),
                        ..cur.clone()
                    });
                }
            }
            _ => {}
        }
    }
    out
}

impl std::fmt::Display for Cycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "({:?} - {:?}) * {:?} + {:?}",
            self.a, self.b, self.c, self.d
        )
    }
}
