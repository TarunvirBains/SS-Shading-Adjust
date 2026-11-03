//! Foliage warmth: leaf and bush materials drawn as `texture × shade × PRIM` (in either cycle
//! order) get a warmer primitive color. The primitive color is a multiplier on the lit texture,
//! so multiplying it by a warm tint moves green leaves toward yellow-green. Only the RGB bytes of
//! their `G_SETPRIMCOLOR` change; the texture, the lighting term, the alpha and every other
//! material stay as they are.

use std::collections::{BTreeMap, HashSet};

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::crc64::crc64;
use crate::dl;
use crate::ground::PrimChange;
use crate::material::{self, In, Material};

/// `[foliage]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Foliage {
    /// Patch the foliage at all.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// The warm multiplier at strength 1 (sRGB bytes as 0..1 per channel; 1 leaves a channel as
    /// it is).
    pub tint: [f32; 3],
    /// 0..1: the share of the tint applied.
    pub strength: f32,
    /// Areas and objects, each a toggle.
    #[serde(default, rename = "scene")]
    pub scenes: Vec<FoliageScene>,
}

fn yes() -> bool {
    true
}

/// The foliage materials of one area or object.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoliageScene {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Display-list resource path glob.
    pub dls: String,
    /// Foliage texture resource paths.
    pub textures: Vec<String>,
}

impl Foliage {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (0.0..=1.0).contains(&self.strength),
            "foliage.strength {} must be within 0..=1",
            self.strength
        );
        ensure!(
            self.tint.iter().all(|v| (0.0..=1.0).contains(v)),
            "foliage.tint {:?} must be within 0..=1 per channel",
            self.tint
        );
        Ok(())
    }

    /// The new primitive color (RGBA, alpha kept) for an old one: every channel is multiplied by
    /// `1 - strength + strength * tint`, so a tint of 1 (or strength 0) changes nothing and the
    /// result never gets lighter.
    pub fn tint(&self, rgba: u32) -> u32 {
        let s = self.strength;
        let channel = |shift: u32, t: f32| {
            let old = ((rgba >> shift) & 0xFF) as f32;
            let new = (old * (1.0 - s + s * t)).round().clamp(0.0, 255.0);
            (new as u32) << shift
        };
        channel(24, self.tint[0])
            | channel(16, self.tint[1])
            | channel(8, self.tint[2])
            | (rgba & 0xFF)
    }
}

/// Whether a material draws `texture × shade × PRIM`: one cycle multiplies a texel by the shade
/// (the lighting term) and the other by PRIM, in either order.
pub fn is_prim_multiply(m: &Material) -> bool {
    let Some((_, w0, w1)) = m.combine else {
        return false;
    };
    let [x, y] = material::color_cycles(w0, w1);
    let times = |c: &material::Cycle, input: In| c.b == In::Zero && c.c == input && c.d == In::Zero;
    let tex_times =
        |c: &material::Cycle, input: In| matches!(c.a, In::Texel0 | In::Texel1) && times(c, input);
    let comb_times = |c: &material::Cycle, input: In| c.a == In::Combined && times(c, input);
    (tex_times(&x, In::Shade) && comb_times(&y, In::Prim))
        || (tex_times(&x, In::Prim) && comb_times(&y, In::Shade))
}

/// Patches the foliage materials' primitive colors of one display list. A primitive color is
/// patched only when every material drawn with it is a foliage material of the `texture × shade
/// × PRIM` form; otherwise it is reported as skipped.
pub fn patch_dl(
    name: &str,
    bytes: &[u8],
    leaves: &HashSet<u64>,
    tint: impl Fn(u32) -> u32,
) -> Result<(Vec<u8>, Vec<PrimChange>)> {
    let cmds = dl::parse(bytes)?;
    let mats = material::materials(&cmds);
    let mut by_prim: BTreeMap<usize, (u32, Vec<&Material>)> = BTreeMap::new();
    for m in &mats {
        if let Some((off, v)) = m.prim {
            by_prim.entry(off).or_insert((v, Vec::new())).1.push(m);
        }
    }
    let mut out = bytes.to_vec();
    let mut changes = Vec::new();
    for (off, (v, users)) in by_prim {
        let leafy: Vec<bool> = users
            .iter()
            .map(|m| m.textures.iter().any(|t| leaves.contains(t)))
            .collect();
        if !leafy.iter().any(|&e| e) {
            continue;
        }
        let tris = users.iter().map(|m| m.tris).sum();
        let skipped = if !leafy.iter().all(|&e| e) {
            "shared with non-foliage materials".to_string()
        } else if !users.iter().all(|m| is_prim_multiply(m)) {
            "combiner is not texture x shade x PRIM".to_string()
        } else {
            String::new()
        };
        let to = if skipped.is_empty() { tint(v) } else { v };
        if to != v {
            ensure!(
                out[off + 3] == dl::G_SETPRIMCOLOR,
                "{name}: no G_SETPRIMCOLOR at {off:#x}"
            );
            out[off + 4..off + 8].copy_from_slice(&to.to_le_bytes());
        }
        changes.push(PrimChange {
            dl: name.to_string(),
            offset: off,
            from: v,
            to,
            skipped,
            tris,
        });
    }
    Ok((out, changes))
}

/// The foliage texture hashes of a scene.
pub fn leaf_set(scene: &FoliageScene) -> HashSet<u64> {
    scene.textures.iter().map(|t| crc64(t)).collect()
}
