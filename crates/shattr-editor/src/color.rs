#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Srgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

fn decode(channel: u8) -> f64 {
    let v = f64::from(channel) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(linear: f64) -> u8 {
    let v = linear.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

fn wrap_degrees(h: f64) -> f64 {
    let h = h.rem_euclid(360.0);
    if h >= 360.0 { 0.0 } else { h }
}

/// Rounding can push e.g. 359.97 to "360.0"; the displayed hue must stay in [0, 360).
fn round_hue(h: f32, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    let r = (f64::from(h) * scale).round() / scale;
    if r >= 360.0 { r - 360.0 } else { r }
}

impl Srgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    #[cfg(test)]
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim();
        let digits = s.strip_prefix('#').unwrap_or(s);
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let nibble = |i: usize| u8::from_str_radix(&digits[i..=i], 16).ok();
        let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
        match digits.len() {
            3 => Some(Self::new(nibble(0)? * 17, nibble(1)? * 17, nibble(2)? * 17)),
            6 => Some(Self::new(byte(0)?, byte(2)?, byte(4)?)),
            _ => None,
        }
    }

    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn hsl(self) -> (f32, f32, f32) {
        let r = f64::from(self.r) / 255.0;
        let g = f64::from(self.g) / 255.0;
        let b = f64::from(self.b) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let d = max - min;
        if d == 0.0 {
            return (0.0, 0.0, (l * 100.0) as f32);
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            60.0 * ((g - b) / d)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        (wrap_degrees(h) as f32, (s * 100.0) as f32, (l * 100.0) as f32)
    }

    pub fn oklch(self) -> (f32, f32, f32) {
        let (r, g, b) = (decode(self.r), decode(self.g), decode(self.b));
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        let ok_l = 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s;
        let ok_a = 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s;
        let ok_b = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s;
        let c = ok_a.hypot(ok_b);
        let h = if c < 1e-4 {
            0.0
        } else {
            wrap_degrees(ok_b.atan2(ok_a).to_degrees())
        };
        (ok_l as f32, c as f32, h as f32)
    }

    pub fn from_oklch(l: f32, c: f32, h: f32) -> Self {
        let l = f64::from(l);
        let c = f64::from(c);
        let h = f64::from(h).to_radians();
        let (a, b) = (c * h.cos(), c * h.sin());
        let l_ = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
        let m_ = (l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
        let s_ = (l - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
        let r = 4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_;
        let g = -1.268_438_004_6 * l_ + 2.609_757_401_1 * m_ - 0.341_319_396_5 * s_;
        let b = -0.004_196_086_3 * l_ - 0.703_418_614_7 * m_ + 1.707_614_701_0 * s_;
        Self::new(encode(r), encode(g), encode(b))
    }

    pub fn luminance(self) -> f64 {
        0.2126 * decode(self.r) + 0.7152 * decode(self.g) + 0.0722 * decode(self.b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Hex,
    Rgb,
    Hsl,
    Oklch,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Hex, Format::Rgb, Format::Hsl, Format::Oklch];

    pub fn label(self) -> &'static str {
        match self {
            Format::Hex => "HEX",
            Format::Rgb => "RGB",
            Format::Hsl => "HSL",
            Format::Oklch => "OKLCH",
        }
    }

    pub fn display(self, c: Srgb) -> String {
        match self {
            Format::Hex => c.hex(),
            Format::Rgb => format!("{} {} {}", c.r, c.g, c.b),
            Format::Hsl => {
                let (h, s, l) = c.hsl();
                format!("{} {}% {}%", round_hue(h, 0), s.round(), l.round())
            }
            Format::Oklch => {
                let (l, ch, h) = c.oklch();
                format!("{:.1}% {:.3} {:.1}", l * 100.0, ch, round_hue(h, 1))
            }
        }
    }

    pub fn css(self, c: Srgb) -> String {
        match self {
            Format::Hex => c.hex(),
            Format::Rgb => format!("rgb({})", self.display(c)),
            Format::Hsl => format!("hsl({})", self.display(c)),
            Format::Oklch => format!("oklch({})", self.display(c)),
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&f| f == self).unwrap_or(0)
    }

    pub fn next(self) -> Format {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }
}

pub fn wcag_ratio(a: Srgb, b: Srgb) -> f64 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WcagPass {
    pub aa_text: bool,
    pub aa_large: bool,
    pub aaa_text: bool,
    pub aaa_large: bool,
}

pub fn wcag_pass(ratio: f64) -> WcagPass {
    WcagPass {
        aa_text: ratio >= 4.5,
        aa_large: ratio >= 3.0,
        aaa_text: ratio >= 7.0,
        aaa_large: ratio >= 4.5,
    }
}

fn apca_y(c: Srgb) -> f64 {
    let ch = |v: u8| (f64::from(v) / 255.0).powf(2.4);
    let y = 0.212_672_9 * ch(c.r) + 0.715_152_2 * ch(c.g) + 0.072_175_0 * ch(c.b);
    if y < 0.022 {
        y + (0.022 - y).powf(1.414)
    } else {
        y
    }
}

pub fn apca_lc(text: Srgb, bg: Srgb) -> f64 {
    let (yt, yb) = (apca_y(text), apca_y(bg));
    if (yb - yt).abs() < 0.0005 {
        return 0.0;
    }
    let out = if yb > yt {
        let sapc = (yb.powf(0.56) - yt.powf(0.57)) * 1.14;
        if sapc < 0.1 { 0.0 } else { sapc - 0.027 }
    } else {
        let sapc = (yb.powf(0.65) - yt.powf(0.62)) * 1.14;
        if sapc > -0.1 { 0.0 } else { sapc + 0.027 }
    };
    out * 100.0
}

/// Contrast is not monotonic in OKLCH L once the colour crosses the background's
/// luminance or hits the gamut clamp, so a coarse scan finds the first passing step
/// before bisection narrows it.
fn search_l(l0: f32, c: f32, h: f32, bg: Srgb, target: f64, dir: f32) -> Option<(f32, Srgb)> {
    const STEPS: u32 = 1000;
    let passes = |l: f32| {
        let s = Srgb::from_oklch(l, c, h);
        (wcag_ratio(s, bg) >= target).then_some(s)
    };
    let mut fail = l0;
    for i in 1..=STEPS {
        let l = (l0 + dir * i as f32 / STEPS as f32).clamp(0.0, 1.0);
        if let Some(mut hit) = passes(l) {
            let mut pass = l;
            for _ in 0..24 {
                let mid = (fail + pass) / 2.0;
                match passes(mid) {
                    Some(s) => {
                        pass = mid;
                        hit = s;
                    }
                    None => fail = mid,
                }
            }
            return Some(((pass - l0).abs(), hit));
        }
        fail = l;
        if l <= 0.0 || l >= 1.0 {
            break;
        }
    }
    None
}

pub fn nearest_passing(fg: Srgb, bg: Srgb, target: f64) -> Option<Srgb> {
    if wcag_ratio(fg, bg) >= target {
        return Some(fg);
    }
    let (l, c, h) = fg.oklch();
    let darker = search_l(l, c, h, bg, target, -1.0);
    let lighter = search_l(l, c, h, bg, target, 1.0);
    match (darker, lighter) {
        (Some(d), Some(li)) => Some(if d.0 <= li.0 { d.1 } else { li.1 }),
        (Some(d), None) => Some(d.1),
        (None, Some(li)) => Some(li.1),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUE: Srgb = Srgb::new(0x25, 0x63, 0xEB);
    const GRAY: Srgb = Srgb::new(0x9C, 0xA3, 0xAF);
    const WHITE: Srgb = Srgb::new(255, 255, 255);
    const BLACK: Srgb = Srgb::new(0, 0, 0);

    #[test]
    fn formats_known_blue() {
        assert_eq!(Format::Hex.display(BLUE), "#2563EB");
        assert_eq!(Format::Rgb.display(BLUE), "37 99 235");
        assert_eq!(Format::Hsl.display(BLUE), "221 83% 53%");
        assert_eq!(Format::Oklch.display(BLUE), "54.6% 0.215 262.9");
        assert_eq!(Format::Hex.css(BLUE), "#2563EB");
        assert_eq!(Format::Rgb.css(BLUE), "rgb(37 99 235)");
        assert_eq!(Format::Hsl.css(BLUE), "hsl(221 83% 53%)");
        assert_eq!(Format::Oklch.css(BLUE), "oklch(54.6% 0.215 262.9)");
        assert_eq!(Format::Oklch.display(Srgb::new(0x15, 0x5D, 0xFC)), "54.7% 0.246 262.9");
    }

    #[test]
    fn achromatic_formats() {
        assert_eq!(Format::Hsl.display(WHITE), "0 0% 100%");
        assert_eq!(Format::Oklch.display(BLACK), "0.0% 0.000 0.0");
        assert_eq!(Format::Oklch.display(WHITE), "100.0% 0.000 0.0");
    }

    #[test]
    fn labels_and_cycle() {
        assert_eq!(Format::ALL.map(Format::label), ["HEX", "RGB", "HSL", "OKLCH"]);
        assert_eq!(Format::Hex.next(), Format::Rgb);
        assert_eq!(Format::Oklch.next(), Format::Hex);
        for f in Format::ALL {
            assert_eq!(f.next().next().next().next(), f);
        }
    }

    #[test]
    fn hex_parsing() {
        assert_eq!(Srgb::from_hex("#2563EB"), Some(BLUE));
        assert_eq!(Srgb::from_hex("2563eb"), Some(BLUE));
        assert_eq!(Srgb::from_hex("#fff"), Some(WHITE));
        assert_eq!(Srgb::from_hex(" #000 "), Some(BLACK));
        for c in [BLUE, GRAY, WHITE, BLACK, Srgb::new(1, 128, 254)] {
            assert_eq!(Srgb::from_hex(&c.hex()), Some(c));
        }
        for junk in ["", "#", "#12", "#12345", "#1234567", "zzzzzz", "#ggg", "##fff", "+1+2+3", "é12"] {
            assert_eq!(Srgb::from_hex(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn wcag_known_ratios() {
        assert_eq!(format!("{:.2}", wcag_ratio(WHITE, BLUE)), "5.17");
        assert_eq!(format!("{:.2}", wcag_ratio(BLUE, WHITE)), "5.17");
        assert_eq!(format!("{:.2}", wcag_ratio(GRAY, WHITE)), "2.54");
        assert!((wcag_ratio(BLACK, WHITE) - 21.0).abs() < 1e-9);
        assert!((wcag_ratio(BLUE, BLUE) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn wcag_thresholds() {
        assert_eq!(
            wcag_pass(5.17),
            WcagPass { aa_text: true, aa_large: true, aaa_text: false, aaa_large: true }
        );
        assert_eq!(
            wcag_pass(4.499),
            WcagPass { aa_text: false, aa_large: true, aaa_text: false, aaa_large: false }
        );
        assert_eq!(
            wcag_pass(2.99),
            WcagPass { aa_text: false, aa_large: false, aaa_text: false, aaa_large: false }
        );
        assert!(wcag_pass(7.0).aaa_text);
    }

    #[test]
    fn apca_known_values() {
        let lc = apca_lc(WHITE, BLUE);
        assert!(-82.0 < lc && lc < -78.0, "{lc}");
        let lc = apca_lc(GRAY, WHITE);
        assert!((47.0..53.0).contains(&lc), "{lc}");
        let lc = apca_lc(BLACK, WHITE);
        assert!((lc - 106.04).abs() < 0.1, "{lc}");
        let lc = apca_lc(WHITE, BLACK);
        assert!((lc + 107.88).abs() < 0.1, "{lc}");
        assert_eq!(apca_lc(BLUE, BLUE), 0.0);
    }

    #[test]
    fn oklch_round_trip() {
        for c in [BLUE, GRAY, WHITE, BLACK, Srgb::new(255, 0, 0), Srgb::new(0, 255, 0), Srgb::new(12, 200, 99)] {
            let (l, ch, h) = c.oklch();
            let back = Srgb::from_oklch(l, ch, h);
            for (x, y) in [(c.r, back.r), (c.g, back.g), (c.b, back.b)] {
                assert!(x.abs_diff(y) <= 1, "{c:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn nearest_passing_darkens_gray_on_white() {
        let s = nearest_passing(GRAY, WHITE, 4.5).expect("reachable");
        let r = wcag_ratio(s, WHITE);
        assert!((4.5..4.8).contains(&r), "{r} {s:?}");
        let (_, _, h0) = GRAY.oklch();
        let (_, _, h1) = s.oklch();
        assert!((h0 - h1).abs() < 10.0, "{h0} {h1}");
    }

    #[test]
    fn nearest_passing_lightens_on_dark_bg() {
        let bg = Srgb::new(0x11, 0x18, 0x27);
        let fg = Srgb::new(0x37, 0x41, 0x51);
        let s = nearest_passing(fg, bg, 4.5).expect("reachable");
        let r = wcag_ratio(s, bg);
        assert!((4.5..4.8).contains(&r), "{r} {s:?}");
        assert!(s.luminance() > fg.luminance());
    }

    #[test]
    fn nearest_passing_identity_and_impossible() {
        assert_eq!(nearest_passing(GRAY, WHITE, 1.0), Some(GRAY));
        assert_eq!(nearest_passing(BLUE, WHITE, 4.5), Some(BLUE));
        assert_eq!(nearest_passing(GRAY, WHITE, 22.0), None);
    }
}
