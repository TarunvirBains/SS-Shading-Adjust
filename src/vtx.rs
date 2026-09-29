//! Vertex colors of unlit materials (they carry the baked light, shade and color): which
//! vertices only target materials draw (`vtx-audit`). A vertex counts as exclusive only when
//! every triangle that uses it, in every display list walked, is drawn by a target material
//! with lighting off; any other material, or a draw the walk cannot resolve, makes it shared.
//!
//! Vertex arrays are `OARR` resources: array type `0x19` at 0x40, the count at 0x44, then 16-byte
//! vertices (position, flag, texture coordinates, RGBA at +12).

use std::collections::{BTreeMap, HashMap};

use crate::dl::{self, Cmd};

/// Where vertices start in an array resource.
pub const VERTICES: usize = 0x48;

pub fn is_vtx_array(bytes: &[u8]) -> bool {
    bytes.len() >= VERTICES
        && &bytes[4..8] == b"RRAO"
        && u32::from_le_bytes(bytes[0x40..0x44].try_into().unwrap()) == 0x19
        && bytes.len()
            == VERTICES + 16 * u32::from_le_bytes(bytes[0x44..0x48].try_into().unwrap()) as usize
}

/// Who draws a vertex.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Use {
    pub target: bool,
    pub other: bool,
}

/// Marks, for one display list, every vertex its triangles use: `(array hash, index) -> Use`.
/// `is_target` decides from the textures in effect and the lighting state (`None`: unknown).
pub fn mark(
    cmds: &[Cmd],
    is_target: &dyn Fn(&[u64], Option<bool>) -> bool,
    uses: &mut HashMap<(u64, usize), Use>,
) {
    let mut slots: [Option<(u64, usize)>; 64] = [None; 64];
    let mut textures: Vec<u64> = Vec::new();
    let mut drew = true;
    let mut lit: Option<bool> = None;
    let mut unknown_slots = false;
    for c in cmds {
        match c.op() {
            dl::G_SETTIMG_OTR_HASH => {
                if drew {
                    textures.clear();
                    drew = false;
                }
                textures.push(c.extra.unwrap_or(0));
            }
            0xD9 => {
                let clear = !(c.w0 | 0xFF00_0000);
                if clear & 0x0002_0000 != 0 {
                    lit = Some(false);
                }
                if c.w1 & 0x0002_0000 != 0 {
                    lit = Some(true);
                }
            }
            dl::G_VTX_OTR_HASH => {
                let n = ((c.w0 >> 12) & 0xFF) as usize;
                let end = ((c.w0 & 0xFF) >> 1) as usize;
                let first = (c.w1 as usize) / 16;
                if let (Some(h), Some(v0)) = (c.extra, end.checked_sub(n)) {
                    for k in 0..n {
                        if v0 + k < 64 {
                            slots[v0 + k] = Some((h, first + k));
                        }
                    }
                }
            }
            dl::G_VTX => {
                // A segment address: the vertices cannot be resolved.
                unknown_slots = true;
                slots = [None; 64];
            }
            dl::G_TRI1 | dl::G_TRI2 | dl::G_QUAD => {
                drew = true;
                let t = is_target(&textures, lit);
                let mut idx = vec![c.w0 >> 16 & 0xFF, c.w0 >> 8 & 0xFF, c.w0 & 0xFF];
                if c.op() != dl::G_TRI1 {
                    idx.extend([c.w1 >> 16 & 0xFF, c.w1 >> 8 & 0xFF, c.w1 & 0xFF]);
                }
                for i in idx {
                    let slot = (i / 2) as usize;
                    if let Some(Some(v)) = slots.get(slot) {
                        let u = uses.entry(*v).or_default();
                        if t && !unknown_slots {
                            u.target = true;
                        } else {
                            u.other = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// The vertices only target materials draw, per array hash.
pub fn exclusive(uses: &HashMap<(u64, usize), Use>) -> BTreeMap<u64, Vec<usize>> {
    let mut out: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (&(h, i), u) in uses {
        if u.target && !u.other {
            out.entry(h).or_default().push(i);
        }
    }
    for v in out.values_mut() {
        v.sort_unstable();
    }
    out
}

/// The color bytes of a vertex.
pub fn color(bytes: &[u8], i: usize) -> [u8; 4] {
    let o = VERTICES + 16 * i + 12;
    [bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]
}
