//! OoT Deeper Water: raises the opacity of Ocarina of Time's translucent water surfaces for Ship
//! of Harkinian by shipping patched display-list resources in a `.o2r` mod. Only the alpha byte
//! of the water materials' primitive color changes; an opacity cap and per-surface values keep
//! swimming, diving and iron-boots sections readable.

pub mod config;
pub mod crc64;
pub mod dl;
pub mod o2r;
pub mod water;
