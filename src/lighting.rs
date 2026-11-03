//! Scene lighting warmth: a scene's `Set_` resource carries its environment light settings (the
//! `SetLightingSettings` command: command id 0x0F, a u32 count, then `count` entries of 22 bytes),
//! one entry per time of day. Daytime entries get warmer ambient, directional-light and fog
//! colors; night and dark interior entries stay as they are. Light directions, the fog distances
//! and every other command of the resource are untouched.
//!
//! Entry layout (bytes): ambient RGB (0..3), light 1 direction (3..6), light 1 color (6..9),
//! light 2 direction (9..12), light 2 color (12..15), fog RGB (15..18), blend rate and fog near
//! (u16, 18..20), fog far (u16, 20..22).

use anyhow::{Result, ensure};
use serde::Deserialize;

/// Bytes per environment light entry.
pub const ENTRY: usize = 22;

/// `[lighting]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lighting {
    /// Patch the lighting at all.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// An entry is daytime when its ambient color's mean brightness (0..1) is at least this.
    pub day_min_luma: f32,
    /// Warm multiplier at strength 1 for the ambient color (1 leaves a channel as it is).
    pub ambient_tint: [f32; 3],
    /// The same for both directional light colors.
    pub light_tint: [f32; 3],
    /// The same for the fog color.
    pub fog_tint: [f32; 3],
    /// 0..1: the share of the tint applied.
    pub strength: f32,
    /// Scenes, each a toggle.
    #[serde(default, rename = "scene")]
    pub scenes: Vec<LightingScene>,
}

fn yes() -> bool {
    true
}

/// The lighting resources of one scene.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingScene {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Scene `Set_` resource path glob.
    pub resources: String,
}

/// One patched (or kept) entry.
#[derive(Debug, Clone)]
pub struct EntryChange {
    pub resource: String,
    /// Index within the lighting list.
    pub index: usize,
    /// Why it was kept as it is (empty: patched).
    pub kept: String,
    pub from: [u8; ENTRY],
    pub to: [u8; ENTRY],
}

impl Lighting {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (0.0..=1.0).contains(&self.strength),
            "lighting.strength {} must be within 0..=1",
            self.strength
        );
        ensure!(
            (0.0..=1.0).contains(&self.day_min_luma),
            "lighting.day_min_luma {} must be within 0..=1",
            self.day_min_luma
        );
        for (what, t) in [
            ("ambient_tint", self.ambient_tint),
            ("light_tint", self.light_tint),
            ("fog_tint", self.fog_tint),
        ] {
            ensure!(
                t.iter().all(|v| (0.0..=1.0).contains(v)),
                "lighting.{what} {t:?} must be within 0..=1 per channel"
            );
        }
        Ok(())
    }

    fn mul(&self, tint: [f32; 3], rgb: &mut [u8]) {
        let s = self.strength;
        for (c, t) in rgb.iter_mut().zip(tint) {
            *c = (*c as f32 * (1.0 - s + s * t)).round().clamp(0.0, 255.0) as u8;
        }
    }

    /// Whether an entry is a daytime one.
    pub fn is_day(&self, entry: &[u8; ENTRY]) -> bool {
        let luma = entry[0..3].iter().map(|&v| v as f32).sum::<f32>() / (3.0 * 255.0);
        luma >= self.day_min_luma
    }

    /// The warmed copy of an entry (never lighter in any color channel).
    pub fn warm(&self, entry: &[u8; ENTRY]) -> [u8; ENTRY] {
        let mut e = *entry;
        self.mul(self.ambient_tint, &mut e[0..3]);
        self.mul(self.light_tint, &mut e[6..9]);
        self.mul(self.light_tint, &mut e[12..15]);
        self.mul(self.fog_tint, &mut e[15..18]);
        e
    }
}

/// The offset and count of the one lighting list of a scene resource, if it has exactly one
/// that parses: the id `0x0F` and a count of 1..=32, then entries whose fog far is a plausible
/// distance.
pub fn find_list(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.get(4..8)? != b"MORO" {
        return None;
    }
    let mut hits = Vec::new();
    for at in 0..bytes.len().saturating_sub(8) {
        if bytes[at..at + 4] != [0x0F, 0, 0, 0] || bytes[at + 5..at + 8] != [0, 0, 0] {
            continue;
        }
        let count = bytes[at + 4] as usize;
        let start = at + 8;
        if !(1..=32).contains(&count) || start + count * ENTRY > bytes.len() {
            continue;
        }
        let plausible = (0..count).all(|i| {
            let far =
                u16::from_le_bytes([bytes[start + i * ENTRY + 20], bytes[start + i * ENTRY + 21]]);
            (1000..=0x7FFF).contains(&far)
        });
        if plausible {
            hits.push((start, count));
        }
    }
    if hits.len() == 1 { Some(hits[0]) } else { None }
}

/// Warms the daytime entries of one scene resource. Resources without exactly one parseable
/// lighting list (and cutscene data) come back unchanged with no entries.
pub fn patch_resource(
    name: &str,
    bytes: &[u8],
    cfg: &Lighting,
) -> Result<(Vec<u8>, Vec<EntryChange>)> {
    let mut out = bytes.to_vec();
    let mut changes = Vec::new();
    if name.contains("CutsceneData") {
        return Ok((out, changes));
    }
    let Some((start, count)) = find_list(bytes) else {
        return Ok((out, changes));
    };
    for i in 0..count {
        let at = start + i * ENTRY;
        let from: [u8; ENTRY] = bytes[at..at + ENTRY].try_into()?;
        let day = cfg.is_day(&from);
        let to = if day { cfg.warm(&from) } else { from };
        out[at..at + ENTRY].copy_from_slice(&to);
        changes.push(EntryChange {
            resource: name.to_string(),
            index: i,
            kept: if day {
                String::new()
            } else {
                "not daytime".to_string()
            },
            from,
            to,
        });
    }
    Ok((out, changes))
}
