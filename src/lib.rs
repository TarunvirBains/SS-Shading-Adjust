//! SS Shading Adjust: engine-side look adjustments for Ship of Harkinian (Ocarina of Time), in
//! the direction of Skyward Sword, shipped as patched game resources in a `.o2r` mod. `water`:
//! deeper water (only the alpha byte of the water materials' primitive color changes; an opacity
//! limit and per-surface values keep swimming, diving and iron-boots sections readable).

pub mod config;
pub mod crc64;
pub mod dl;
pub mod glob;
pub mod ground;
pub mod material;
pub mod o2r;
pub mod water;
