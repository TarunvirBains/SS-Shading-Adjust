//! Ground tint: earth materials drawn as `texture × shade × PRIM` (the scene's lighting times a
//! primitive-color multiplier) get a warmer primitive color. Only the RGB bytes of their
//! `G_SETPRIMCOLOR` change; the lighting term, the texture, the alpha and every other material
//! stay as they are.
//!
//! The ground textures are gray, so on screen the color is `texture × shade × prim` and, in
//! OKLab, a gray texture of lightness `Lt` scales the primitive color's lightness and chroma by
//! `Lt` and keeps its hue. The new primitive color keeps the old one's OKLab lightness exactly
//! (the path stays as light as vanilla under every light) and moves its a/b toward the target
//! hue at the target on-screen chroma, `strength` of the way.

use std::collections::{BTreeMap, HashSet};

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::crc64::crc64;
use crate::dl;
use crate::material::{self, In, Material};

/// `[ground]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ground {
    /// Patch the ground at all.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Target OKLCH hue on screen (degrees).
    pub target_hue: f32,
    /// Target OKLCH chroma on screen.
    pub target_chroma: f32,
    /// 0..1: how far the primitive color moves toward the target.
    pub strength: f32,
    /// Scenes (areas), each a toggle.
    #[serde(default, rename = "scene")]
    pub scenes: Vec<GroundScene>,
}

fn yes() -> bool {
    true
}

/// The earth materials of one area.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundScene {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Display-list resource path glob.
    pub dls: String,
    /// Earth texture resource paths.
    pub textures: Vec<String>,
    /// Mean OKLab lightness of these textures as drawn (gray: it scales the primitive color's
    /// lightness and chroma on screen).
    pub texture_lightness: f32,
}

impl Ground {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (0.0..=1.0).contains(&self.strength),
            "ground.strength {} must be within 0..=1",
            self.strength
        );
        ensure!(
            (0.0..=0.3).contains(&self.target_chroma),
            "ground.target_chroma {} must be within 0..=0.3",
            self.target_chroma
        );
        for s in &self.scenes {
            ensure!(
                s.texture_lightness > 0.0 && s.texture_lightness <= 1.0,
                "ground scene {}: texture_lightness must be within (0, 1]",
                s.name
            );
        }
        Ok(())
    }

    /// The new primitive color (RGBA, alpha kept) for an old one.
    pub fn tint(&self, scene: &GroundScene, rgba: u32) -> u32 {
        let c = |shift: u32| ((rgba >> shift) & 0xFF) as f32 / 255.0;
        let lab = srgb_to_oklab([c(24), c(16), c(8)]);
        let chroma = self.target_chroma / scene.texture_lightness;
        let h = self.target_hue.to_radians();
        let s = self.strength;
        let mut ab = [
            lab[1] + (chroma * h.cos() - lab[1]) * s,
            lab[2] + (chroma * h.sin() - lab[2]) * s,
        ];
        // Out of gamut: reduce chroma until it fits (lightness and hue stay).
        let mut rgb = oklab_to_srgb([lab[0], ab[0], ab[1]]);
        for _ in 0..32 {
            if rgb.iter().all(|v| (-1e-4..=1.0 + 1e-4).contains(v)) {
                break;
            }
            ab = [ab[0] * 0.95, ab[1] * 0.95];
            rgb = oklab_to_srgb([lab[0], ab[0], ab[1]]);
        }
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        (byte(rgb[0]) << 24) | (byte(rgb[1]) << 16) | (byte(rgb[2]) << 8) | (rgba & 0xFF)
    }
}

/// Whether a material draws `texture × shade × PRIM`: one cycle is `(COMBINED - 0) * PRIM + 0`
/// and the other multiplies a texel by the shade (the lighting term).
pub fn is_prim_multiply(m: &Material) -> bool {
    let Some((_, w0, w1)) = m.combine else {
        return false;
    };
    let [x, y] = material::color_cycles(w0, w1);
    let times_prim = |c: &material::Cycle| {
        c.a == In::Combined && c.b == In::Zero && c.c == In::Prim && c.d == In::Zero
    };
    let texel_shade = |c: &material::Cycle| {
        matches!(c.a, In::Texel0 | In::Texel1)
            && c.b == In::Zero
            && c.c == In::Shade
            && c.d == In::Zero
    };
    texel_shade(&x) && times_prim(&y)
}

/// One patched (or skipped) primitive color.
#[derive(Debug, Clone)]
pub struct PrimChange {
    pub dl: String,
    pub offset: usize,
    pub from: u32,
    pub to: u32,
    /// Why it was not patched (empty: patched).
    pub skipped: String,
    /// Triangles drawn with it.
    pub tris: usize,
}

/// Patches the earth materials' primitive colors of one display list. A primitive color is
/// patched only when every material drawn with it is an earth material of the `texture × shade
/// × PRIM` form; otherwise it is reported as skipped.
pub fn patch_dl(
    name: &str,
    bytes: &[u8],
    earth: &HashSet<u64>,
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
        let earthy: Vec<bool> = users
            .iter()
            .map(|m| m.textures.iter().any(|t| earth.contains(t)))
            .collect();
        if !earthy.iter().any(|&e| e) {
            continue;
        }
        let tris = users.iter().map(|m| m.tris).sum();
        let skipped = if !earthy.iter().all(|&e| e) {
            "shared with non-earth materials".to_string()
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

/// The earth texture hashes of a scene.
pub fn earth_set(scene: &GroundScene) -> HashSet<u64> {
    scene.textures.iter().map(|t| crc64(t)).collect()
}

// OKLab (Björn Ottosson), gamma sRGB in 0..1.
fn to_lin(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_lin(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.max(0.0).powf(1.0 / 2.4) - 0.055
    }
}

pub fn srgb_to_oklab(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(to_lin);
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

pub fn oklab_to_srgb([ll, a, b]: [f32; 3]) -> [f32; 3] {
    let l = (ll + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (ll - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (ll - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
    .map(from_lin)
}
