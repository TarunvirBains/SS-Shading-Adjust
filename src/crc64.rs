//! The resource-path hash libultraship uses in display lists: CRC-64/ECMA-182 (polynomial
//! 0x42F0E1EBA9EA3693, not reflected, initial value all ones, no final xor).

const POLY: u64 = 0x42F0_E1EB_A9EA_3693;

const fn table() -> [u64; 256] {
    let mut t = [0u64; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u64) << 56;
        let mut k = 0;
        while k < 8 {
            c = if c >> 63 != 0 {
                (c << 1) ^ POLY
            } else {
                c << 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static TABLE: [u64; 256] = table();

/// The hash of a resource path (e.g. `scenes/shared/spot04_scene/spot04_room_0Tex_018A90`).
pub fn crc64(s: &str) -> u64 {
    let mut c = !0u64;
    for &b in s.as_bytes() {
        c = (c << 8) ^ TABLE[(((c >> 56) ^ b as u64) & 0xff) as usize];
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_value() {
        // CRC-64/WE (same polynomial, initial value all ones, final xor all ones) of "123456789"
        // is 0x62EC59E3F1A4F00A.
        assert_eq!(crc64("123456789") ^ !0, 0x62EC_59E3_F1A4_F00A);
    }
}
