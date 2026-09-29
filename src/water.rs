//! Finding the water surfaces and patching their opacity.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail, ensure};

use crate::config::Water;
use crate::crc64::crc64;
use crate::dl;
use crate::o2r::Archive;

/// A translucent material (primitive alpha below 255 feeding the output alpha).
#[derive(Debug, Clone)]
pub struct Material {
    pub dl: String,
    pub offset: usize,
    pub alpha: u8,
    /// Texture resource paths (or hashes when unknown).
    pub textures: Vec<String>,
    /// Draws a configured water texture.
    pub water: bool,
}

/// Resource paths by hash, for every entry of the archive.
pub fn names_by_hash(archive: &Archive) -> HashMap<u64, String> {
    archive
        .names()
        .into_iter()
        .map(|n| (crc64(&n), n))
        .collect()
}

fn water_set(config_textures: &[String]) -> HashSet<u64> {
    config_textures.iter().map(|t| crc64(t)).collect()
}

/// Every translucent material of every display list in the archive (entries matching `filter`).
pub fn translucent_materials(
    archive: &mut Archive,
    water_textures: &[String],
    filter: impl Fn(&str) -> bool,
) -> Result<Vec<Material>> {
    let names = names_by_hash(archive);
    let water = water_set(water_textures);
    let mut out = Vec::new();
    for name in archive.names() {
        if !filter(&name) {
            continue;
        }
        let bytes = archive.read(&name)?;
        if !dl::is_dl(&bytes) {
            continue;
        }
        let cmds = dl::parse(&bytes).map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        for p in dl::prim_alphas(&cmds) {
            if p.alpha == 255 || !p.used {
                continue;
            }
            out.push(Material {
                dl: name.clone(),
                offset: p.offset,
                alpha: p.alpha,
                water: p.textures.iter().any(|h| water.contains(h)),
                textures: p
                    .textures
                    .iter()
                    .map(|h| names.get(h).cloned().unwrap_or_else(|| format!("{h:016x}")))
                    .collect(),
            });
        }
    }
    Ok(out)
}

/// One patched primitive color.
#[derive(Debug, Clone)]
pub struct Change {
    pub dl: String,
    pub offset: usize,
    pub from: u8,
    pub to: u8,
}

/// Patches one display list: every water material's primitive alpha becomes `target(current)`.
/// Returns the new bytes and the changes; only alpha bytes differ from the input.
pub fn patch_dl(
    name: &str,
    bytes: &[u8],
    water: &HashSet<u64>,
    target: impl Fn(u8) -> u8,
) -> Result<(Vec<u8>, Vec<Change>)> {
    let cmds = dl::parse(bytes)?;
    let mut out = bytes.to_vec();
    let mut changes = Vec::new();
    for p in dl::prim_alphas(&cmds) {
        if p.alpha == 255 || !p.used || !p.textures.iter().any(|h| water.contains(h)) {
            continue;
        }
        let to = target(p.alpha);
        if to != p.alpha {
            dl::set_prim_alpha(&mut out, p.offset, to)?;
        }
        changes.push(Change {
            dl: name.to_string(),
            offset: p.offset,
            from: p.alpha,
            to,
        });
    }
    Ok((out, changes))
}

/// The opacity that maps to `max_alpha`: the config's `a_max`, else the most opaque water
/// material of the configured surfaces below `max_alpha` (water at or above it never sets it).
pub fn a_max(archive: &mut Archive, config: &Water) -> Result<f32> {
    if let Some(a) = config.a_max {
        return Ok(a);
    }
    let water = water_set(&config.water_textures);
    let mut best = 0u8;
    for s in &config.surfaces {
        let bytes = archive.read(&s.dl)?;
        let (_, changes) = patch_dl(&s.dl, &bytes, &water, |a| a)?;
        for c in changes {
            if !config.at_or_above_max(c.from as f32 / 255.0) {
                best = best.max(c.from);
            }
        }
    }
    ensure!(best > 0, "no water material below max_alpha");
    Ok(best as f32 / 255.0)
}

/// Archive entries: resource path and bytes.
pub type Entries = Vec<(String, Vec<u8>)>;

/// The mod's entries: every configured surface's display list, patched. Also returns `a_max`.
pub fn build(archive: &mut Archive, config: &Water) -> Result<(Entries, Vec<Change>, f32)> {
    let a_max = a_max(archive, config)?;
    let water = water_set(&config.water_textures);
    let (mut entries, mut all) = (Vec::new(), Vec::new());
    for s in &config.surfaces {
        let bytes = archive.read(&s.dl)?;
        let (out, changes) = patch_dl(&s.dl, &bytes, &water, |a| config.target(s, a, a_max))?;
        if changes.is_empty() {
            bail!("{}: no translucent water material", s.dl);
        }
        all.extend(changes);
        if out != bytes {
            entries.push((s.dl.clone(), out));
        }
    }
    Ok((entries, all, a_max))
}
