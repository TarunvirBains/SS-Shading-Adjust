//! The water config: which textures are water, the opacity mapping and its limit, and the
//! patched surfaces.
//!
//! Every patched water material's opacity `a` (its primitive alpha) is scaled proportionally,
//! `a * max_alpha / a_max`, where `a_max` is the most opaque patched water: every surface gets
//! more opaque, the ratios between surfaces (the water's dynamic range) stay, and the most
//! opaque water lands on `max_alpha`. Water already at or above `max_alpha` keeps its own
//! value; nothing is ever made less opaque. A surface's own `max_alpha` (gameplay visibility)
//! caps its result.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The most opaque any water becomes (0..1); per-surface limits may only be lower.
    pub max_alpha: f32,
    pub mapping: Mapping,
    /// The opacity that maps to `max_alpha`; unset: the most opaque patched water material
    /// below `max_alpha` (measured from the game archive).
    pub a_max: Option<f32>,
    /// Resource paths of the water textures (a material drawing one of them is a water surface).
    pub water_textures: Vec<String>,
    /// The patched surfaces.
    #[serde(default, rename = "surface")]
    pub surfaces: Vec<Surface>,
}

/// How opacities are mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mapping {
    /// `a * max_alpha / a_max`: ratios kept, every surface lifted.
    Proportional,
}

/// A display list whose water materials get the mapped opacity.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    /// Resource path of the display list.
    pub dl: String,
    /// At most this opacity (0..1; gameplay needs to see below or through).
    pub max_alpha: Option<f32>,
    /// Why this surface is patched, and why its limit.
    #[serde(default)]
    pub reason: String,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let c: Self =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        c.validate()?;
        Ok(c)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.max_alpha > 0.0 && self.max_alpha <= 1.0,
            "max_alpha {} must be within (0, 1]",
            self.max_alpha
        );
        if let Some(a) = self.a_max {
            ensure!(
                a > 0.0 && a <= self.max_alpha,
                "a_max {a} must be within (0, max_alpha]"
            );
        }
        for s in &self.surfaces {
            if let Some(a) = s.max_alpha {
                ensure!(
                    (0.0..=self.max_alpha).contains(&a),
                    "{}: max_alpha {a} must be within 0..=max_alpha ({})",
                    s.dl,
                    self.max_alpha
                );
            }
        }
        let mut dls: Vec<&str> = self.surfaces.iter().map(|s| s.dl.as_str()).collect();
        dls.sort();
        ensure!(
            dls.windows(2).all(|w| w[0] != w[1]),
            "a display list is listed twice"
        );
        Ok(())
    }

    /// The scale for the most opaque water `a_max`.
    pub fn scale(&self, a_max: f32) -> f32 {
        match self.mapping {
            Mapping::Proportional => self.max_alpha / a_max.max(1e-3),
        }
    }

    /// Whether water of opacity `a` is already at or above `max_alpha` (within half a percent,
    /// the precision of an alpha byte): it keeps its own value and never sets the scale.
    pub fn at_or_above_max(&self, a: f32) -> bool {
        a >= self.threshold()
    }

    /// `max_alpha` less half a percent (the precision of an alpha byte): water at or above it is
    /// unchanged, and nothing is mapped above it, so the order of all water is kept.
    pub fn threshold(&self) -> f32 {
        self.max_alpha - 0.005
    }

    /// The shared mapping of an opacity (0..1): scaled, at most `max_alpha`, never below the
    /// input; water already at or above `max_alpha` is unchanged.
    pub fn mapped(&self, a: f32, a_max: f32) -> f32 {
        if self.at_or_above_max(a) {
            return a;
        }
        (a * self.scale(a_max)).min(self.threshold()).max(a)
    }

    /// The opacity (0..1) a material of opacity `a` on `surface` gets: the mapped opacity, at
    /// most the surface's `max_alpha`, never below `a`.
    pub fn final_alpha(&self, surface: Option<&Surface>, a: f32, a_max: f32) -> f32 {
        let m = self.mapped(a, a_max);
        surface
            .and_then(|s| s.max_alpha)
            .map_or(m, |x| m.min(x))
            .max(a)
    }

    /// The primitive alpha byte a material with alpha byte `current` gets.
    pub fn target(&self, surface: &Surface, current: u8, a_max: f32) -> u8 {
        let a = self.final_alpha(Some(surface), current as f32 / 255.0, a_max);
        ((a * 255.0).round() as u8).max(current)
    }
}
