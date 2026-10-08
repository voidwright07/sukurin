//! Turning an HDR desktop into the colours SDR apps actually drew.
//!
//! With HDR on, Windows composes the desktop in scRGB: linear light where
//! 1.0 is 80 nits. SDR apps are placed at the user's "SDR content
//! brightness" white (4.0 at 320 nits). Asking duplication for 8-bit output
//! makes Windows map 1.0, not that white, to 255, so every SDR colour comes
//! out several times too bright and highlights clip. Capturing FP16 and
//! dividing by the real SDR white restores the original sRGB values.

/// Every FP16 bit pattern mapped to an 8-bit sRGB channel, for one SDR white
/// level. A 64 KB table turns the per-pixel work into three lookups.
pub struct ToneMap {
    white: f32,
    table: Vec<u8>,
}

impl ToneMap {
    /// `white` is the SDR white level in scRGB units (nits / 80).
    pub fn new(white: f32) -> Self {
        let white = if white.is_finite() && white > 0.0 { white } else { 1.0 };
        let table = (0..=u16::MAX).map(|h| encode_srgb(f16_to_f32(h) / white)).collect();
        Self { white, table }
    }

    pub fn white(&self) -> f32 {
        self.white
    }

    /// One row of RGBA16F pixels to BGRA8, alpha forced opaque.
    pub fn convert_row(&self, rgba16f: &[u8], bgra8: &mut [u8]) {
        for (src, dst) in rgba16f.as_chunks::<8>().0.iter().zip(bgra8.as_chunks_mut::<4>().0.iter_mut()) {
            let ch = |i: usize| self.table[u16::from_le_bytes([src[i], src[i + 1]]) as usize];
            dst[0] = ch(4);
            dst[1] = ch(2);
            dst[2] = ch(0);
            dst[3] = 255;
        }
    }
}

pub fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1f) as i32;
    let man = (h & 0x3ff) as f32;
    sign * match exp {
        0 => man * 2f32.powi(-24),
        31 => f32::INFINITY,
        e => (1.0 + man / 1024.0) * 2f32.powi(e - 15),
    }
}

/// Linear light to an sRGB-encoded byte. Values above SDR white clip, which
/// only affects genuinely HDR content (games, HDR video).
fn encode_srgb(linear: f32) -> u8 {
    let l = if linear.is_nan() { 0.0 } else { linear.clamp(0.0, 1.0) };
    let v = if l <= 0.003_130_8 { 12.92 * l } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (v * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_srgb(b: u8) -> f32 {
        let v = b as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    }

    fn f32_to_f16(v: f32) -> u16 {
        (0..=u16::MAX)
            .filter(|h| f16_to_f32(*h).is_finite() && f16_to_f32(*h) >= 0.0)
            .min_by(|a, b| (f16_to_f32(*a) - v).abs().total_cmp(&(f16_to_f32(*b) - v).abs()))
            .unwrap()
    }

    #[test]
    fn decodes_half_floats() {
        assert_eq!(f16_to_f32(0x3C00), 1.0);
        assert_eq!(f16_to_f32(0x4400), 4.0);
        assert_eq!(f16_to_f32(0xC000), -2.0);
        assert_eq!(f16_to_f32(0x0000), 0.0);
    }

    /// The case seen on a real 320-nit desktop: YouTube's #0F0F0F came out
    /// of 8-bit duplication as #262626. Through the tone map it must be
    /// #0F0F0F again, and SDR white must stay white without clipping grey.
    #[test]
    fn sdr_colours_round_trip_at_the_users_white_level() {
        let white = 4.0;
        let map = ToneMap::new(white);
        for byte in [0x0F_u8, 0x26, 0x80, 0xC8, 0xFF] {
            let scrgb = decode_srgb(byte) * white;
            let h = f32_to_f16(scrgb).to_le_bytes();
            let px = [h[0], h[1], h[0], h[1], h[0], h[1], 0, 0x3C];
            let mut out = [0u8; 4];
            map.convert_row(&px, &mut out);
            assert_eq!(out, [byte, byte, byte, 255], "#{byte:02X} survives the HDR desktop");
        }
    }

    #[test]
    fn channels_land_in_bgra_order() {
        let map = ToneMap::new(1.0);
        let one = 0x3C00u16.to_le_bytes();
        let zero = [0, 0];
        let px = [one[0], one[1], zero[0], zero[1], zero[0], zero[1], one[0], one[1]];
        let mut out = [0u8; 4];
        map.convert_row(&px, &mut out);
        assert_eq!(out, [0, 0, 255, 255], "pure red in RGBA16F is red in BGRA8");
    }

    #[test]
    fn a_bogus_white_level_falls_back_to_the_hdr_reference() {
        assert_eq!(ToneMap::new(0.0).white(), 1.0);
        assert_eq!(ToneMap::new(f32::NAN).white(), 1.0);
    }
}
