//! Lava glow: Skyward Sword's Eldin lava is a glowing orange-yellow; Ocarina of Time's (and the
//! texture packs') is a deep red that reads flat. The lava textures stay as they are (a texture
//! restyler owns them); the lava materials' combiner gains an additive glow color after the
//! lighting term, so the red lava moves toward orange and its bright veins toward yellow while
//! every shading difference (vertex colors, scene lights, the animated crust blend) stays.
//!
//! Two combiner forms are handled, only on combiner and primitive-color commands that lava
//! materials use exclusively:
//! - cycle 2 `(COMBINED - 0) * X + 0` (X the shade or the environment color): `d` becomes
//!   PRIM, and PRIM (unused by the color path before) is set to the glow;
//! - cycle 2 `(COMBINED - 0) * PRIM + 0` with an opaque white PRIM (identity): it becomes
//!   `(PRIM - 0) * PRIM_ALPHA + COMBINED` with PRIM set to the glow (alpha kept at 1).

use std::collections::{BTreeMap, HashSet};

use anyhow::{Result, ensure};
use serde::Deserialize;

use crate::crc64::crc64;
use crate::dl;
use crate::material::{self, In, Material};

/// `[lava]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lava {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// The glow added at strength 1 (linear 0..1 per channel, as the combiner adds it).
    pub glow: [f32; 3],
    /// 0..1: the share of the glow added.
    pub strength: f32,
    #[serde(default, rename = "scene")]
    pub scenes: Vec<LavaScene>,
}

fn yes() -> bool {
    true
}

/// The lava materials of one area.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LavaScene {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Display-list resource path glob.
    pub dls: String,
    /// Lava texture resource paths.
    pub textures: Vec<String>,
}

impl Lava {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (0.0..=1.0).contains(&self.strength),
            "lava.strength {} must be within 0..=1",
            self.strength
        );
        ensure!(
            self.glow.iter().all(|v| (0.0..=0.5).contains(v)),
            "lava.glow channels must be within 0..=0.5 (lava stays readable against rock)"
        );
        Ok(())
    }

    /// The glow as a primitive color (RGB bytes, alpha 0xFF).
    pub fn prim(&self) -> u32 {
        let b = |v: f32| ((v * self.strength).clamp(0.0, 1.0) * 255.0).round() as u32;
        (b(self.glow[0]) << 24) | (b(self.glow[1]) << 16) | (b(self.glow[2]) << 8) | 0xFF
    }
}

pub fn texture_set(scene: &LavaScene) -> HashSet<u64> {
    scene.textures.iter().map(|t| crc64(t)).collect()
}

/// Which form a material's combiner has (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Cycle 2 `(COMBINED - 0) * X + 0`, X not PRIM, PRIM unused in the color path.
    TimesX,
    /// Cycle 2 `(COMBINED - 0) * PRIM + 0`.
    TimesPrim,
}

pub fn form(m: &Material) -> Option<Form> {
    let (_, w0, w1) = m.combine?;
    let [x, y] = material::color_cycles(w0, w1);
    if y.a != In::Combined || y.b != In::Zero || y.d != In::Zero {
        return None;
    }
    let uses_prim = |c: &material::Cycle| [c.a, c.b, c.c, c.d].contains(&In::Prim);
    match y.c {
        In::Prim if !uses_prim(&x) => Some(Form::TimesPrim),
        In::Shade | In::Env if !uses_prim(&x) => Some(Form::TimesX),
        _ => None,
    }
}

/// The combiner words with the glow added.
pub fn glow_combiner(form: Form, w0: u32, w1: u32) -> (u32, u32) {
    match form {
        // d1 (w1 bits 8..6) = PRIM (3).
        Form::TimesX => (w0, (w1 & !(0x7 << 6)) | (3 << 6)),
        // a1 (w0 8..5) = PRIM, c1 (w0 4..0) = PRIM_ALPHA (10), b1 (w1 27..24) = ZERO (15),
        // d1 (w1 8..6) = COMBINED (0).
        Form::TimesPrim => (
            (w0 & !0x1FF) | (3 << 5) | 10,
            (w1 & !(0xF << 24) & !(0x7 << 6)) | (15 << 24),
        ),
    }
}

/// One lava material patch (or why it was skipped).
#[derive(Debug, Clone)]
pub struct LavaChange {
    pub dl: String,
    pub combine: usize,
    pub prim: usize,
    pub form: Option<Form>,
    pub tris: usize,
    pub skipped: String,
}

/// Adds the glow to the lava materials of one display list. A material is patched only when
/// its combiner and primitive-color commands are used by lava materials alone.
pub fn patch_dl(
    name: &str,
    bytes: &[u8],
    lava: &HashSet<u64>,
    prim: u32,
) -> Result<(Vec<u8>, Vec<LavaChange>)> {
    let cmds = dl::parse(bytes)?;
    let mats = material::materials(&cmds);
    let is_lava = |m: &Material| m.textures.iter().any(|t| lava.contains(t));
    // Every material using a combiner (or primitive color) command.
    let mut by_comb: BTreeMap<usize, Vec<&Material>> = BTreeMap::new();
    let mut by_prim: BTreeMap<usize, Vec<&Material>> = BTreeMap::new();
    for m in &mats {
        if let Some((o, ..)) = m.combine {
            by_comb.entry(o).or_default().push(m);
        }
        if let Some((o, _)) = m.prim {
            by_prim.entry(o).or_default().push(m);
        }
    }
    let mut out = bytes.to_vec();
    let mut changes = Vec::new();
    let mut done: HashSet<usize> = HashSet::new();
    for m in mats.iter().filter(|m| is_lava(m)) {
        let (Some((co, w0, w1)), Some((po, pv))) = (m.combine, m.prim) else {
            changes.push(LavaChange {
                dl: name.into(),
                combine: 0,
                prim: 0,
                form: None,
                tris: m.tris,
                skipped: "combiner or primitive color set outside the list".into(),
            });
            continue;
        };
        if !done.insert(co) {
            continue;
        }
        let f = form(m);
        let exclusive = by_comb[&co].iter().all(|x| is_lava(x) && form(x) == f)
            && by_prim[&po].iter().all(|x| is_lava(x));
        let skipped = if f.is_none() {
            "combiner form not handled".to_string()
        } else if !exclusive {
            "combiner or primitive color shared with other materials".to_string()
        } else if f == Some(Form::TimesPrim) && pv != 0xFFFF_FFFF {
            "primitive color is not opaque white".to_string()
        } else {
            String::new()
        };
        if skipped.is_empty() {
            let (n0, n1) = glow_combiner(f.unwrap(), w0, w1);
            ensure!(
                out[co + 3] == dl::G_SETCOMBINE,
                "{name}: no G_SETCOMBINE at {co:#x}"
            );
            ensure!(
                out[po + 3] == dl::G_SETPRIMCOLOR,
                "{name}: no G_SETPRIMCOLOR at {po:#x}"
            );
            out[co..co + 4].copy_from_slice(&((0xFC << 24) | n0).to_le_bytes());
            out[co + 4..co + 8].copy_from_slice(&n1.to_le_bytes());
            // RGB from the glow; the alpha byte stays.
            let p = (prim & 0xFFFF_FF00) | (pv & 0xFF);
            out[po + 4..po + 8].copy_from_slice(&p.to_le_bytes());
        }
        changes.push(LavaChange {
            dl: name.into(),
            combine: co,
            prim: po,
            form: f,
            tris: by_comb[&co].iter().map(|x| x.tris).sum(),
            skipped,
        });
    }
    Ok((out, changes))
}
