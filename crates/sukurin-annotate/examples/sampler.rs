//! Renders one of every annotation kind over a synthetic app screenshot.
//! Usage: cargo run --example sampler -- out.png

use sukurin_annotate::*;
use sukurin_core::{Bitmap, Rgba};

const W: u32 = 1600;
const H: u32 = 1000;

fn flat(w: u32, h: u32, c: Rgba) -> Bitmap {
    Bitmap::from_rgba(w, h, [c.r, c.g, c.b, 255].repeat((w * h) as usize))
}

fn solid(color: Rgba, radius: f32) -> Style {
    Style { color, fill: Fill::Solid, width: 1.0, corner_radius: radius, ..Style::default() }
}

fn ink(color: Rgba, size: f32, bold: bool) -> Style {
    Style { color, font_size: size, bold, text_background: TextBackground::None, ..Style::default() }
}

/// A light dashboard with a dark sidebar, built from the crate's own shapes.
fn base_ui() -> Bitmap {
    let mut d = Document::new(flat(W, H, Rgba::opaque(0xF4, 0xF5, 0xF7)), 1.0);
    let border = Rgba::opaque(0xE3, 0xE5, 0xE9);
    let text = Rgba::opaque(0x1F, 0x23, 0x2B);
    let muted = Rgba::opaque(0x6B, 0x72, 0x80);
    let rect = |d: &mut Document, x, y, w, h, c, r| d.add(Shape::Rect { rect: RectF::new(x, y, w, h) }, solid(c, r));
    let say = |d: &mut Document, x, y, s: &str, st| d.add(Shape::Text { origin: Pt::new(x, y), text: s.into() }, st);

    rect(&mut d, 0.0, 0.0, 240.0, 1000.0, Rgba::opaque(0x15, 0x18, 0x1E), 0.0);
    say(&mut d, 28.0, 26.0, "Northwind", ink(Rgba::opaque(0xFF, 0xFF, 0xFF), 22.0, true));
    for (i, item) in ["Overview", "Projects", "Deployments", "Billing", "API keys", "Settings"].iter().enumerate() {
        let y = 96.0 + i as f32 * 44.0;
        if i == 4 {
            rect(&mut d, 14.0, y - 8.0, 212.0, 38.0, Rgba::opaque(0x26, 0x2B, 0x35), 8.0);
        }
        say(&mut d, 30.0, y, item, ink(Rgba::opaque(0xC9, 0xCE, 0xD6), 17.0, i == 4));
    }
    rect(&mut d, 240.0, 0.0, 1360.0, 64.0, Rgba::opaque(0xFF, 0xFF, 0xFF), 0.0);
    rect(&mut d, 240.0, 64.0, 1360.0, 1.0, border, 0.0);
    say(&mut d, 280.0, 20.0, "API keys", ink(text, 22.0, true));
    rect(&mut d, 1380.0, 14.0, 180.0, 36.0, Rgba::opaque(0x25, 0x63, 0xEB), 8.0);
    say(&mut d, 1404.0, 22.0, "Create new key", ink(Rgba::opaque(0xFF, 0xFF, 0xFF), 16.0, true));

    let card = |d: &mut Document, x, y, w, h| {
        rect(d, x, y, w, h, border, 12.0);
        rect(d, x + 1.0, y + 1.0, w - 2.0, h - 2.0, Rgba::opaque(0xFF, 0xFF, 0xFF), 11.0);
    };
    card(&mut d, 280.0, 100.0, 600.0, 330.0);
    say(&mut d, 308.0, 124.0, "Production key", ink(text, 20.0, true));
    say(&mut d, 308.0, 160.0, "Used by 3 services. Rotate every 90 days.", ink(muted, 16.0, false));
    rect(&mut d, 308.0, 200.0, 544.0, 52.0, Rgba::opaque(0xF6, 0xF7, 0xF9), 8.0);
    say(&mut d, 326.0, 214.0, "sk_demo_7Qw3Xn9Lp2Rt8Vk4Mz6Bc1Hd", ink(text, 18.0, false));
    say(&mut d, 308.0, 276.0, "Owner  priya.raman@northwind.io", ink(text, 16.0, false));
    say(&mut d, 308.0, 306.0, "Created  12 Mar 2026, 09:41", ink(muted, 16.0, false));
    rect(&mut d, 308.0, 360.0, 150.0, 40.0, Rgba::opaque(0x25, 0x63, 0xEB), 8.0);
    say(&mut d, 336.0, 369.0, "Rotate key", ink(Rgba::opaque(0xFF, 0xFF, 0xFF), 16.0, true));

    card(&mut d, 920.0, 100.0, 640.0, 330.0);
    say(&mut d, 948.0, 124.0, "Usage this month", ink(text, 20.0, true));
    for i in 0..12 {
        let h = 40.0 + ((i * 37) % 11) as f32 * 14.0;
        rect(&mut d, 960.0 + i as f32 * 46.0, 400.0 - h, 28.0, h, Rgba::opaque(0x60, 0xA5, 0xFA), 4.0);
    }

    card(&mut d, 280.0, 470.0, 1280.0, 490.0);
    say(&mut d, 308.0, 494.0, "Members", ink(text, 20.0, true));
    let people = [
        ("Priya Raman", "priya.raman@northwind.io", "Admin"),
        ("Tomasz Nowak", "t.nowak@northwind.io", "Developer"),
        ("Aiko Tanaka", "aiko@northwind.io", "Developer"),
        ("Luis Ortega", "luis.ortega@northwind.io", "Billing"),
        ("Grace Mensah", "grace.m@northwind.io", "Viewer"),
        ("Omar Haddad", "omar.haddad@northwind.io", "Developer"),
    ];
    for (i, (name, mail, role)) in people.iter().enumerate() {
        let y = 548.0 + i as f32 * 62.0;
        rect(&mut d, 300.0, y - 14.0, 1240.0, 1.0, border, 0.0);
        let hue = [0x3B82F6, 0x10B981, 0xF59E0B, 0xEF4444, 0x8B5CF6, 0x14B8A6][i];
        let c = Rgba::opaque((hue >> 16) as u8, (hue >> 8) as u8, hue as u8);
        d.add(Shape::Oval { rect: RectF::new(308.0, y, 34.0, 34.0) }, solid(c, 0.0));
        say(&mut d, 358.0, y + 7.0, name, ink(text, 16.0, true));
        say(&mut d, 640.0, y + 7.0, mail, ink(muted, 16.0, false));
        say(&mut d, 1040.0, y + 7.0, role, ink(text, 16.0, false));
    }
    export(&d)
}

fn logo() -> Bitmap {
    let (w, h) = (120u32, 120u32);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f32 - 60.0, y as f32 - 60.0);
            let inside = dx.abs() + dy.abs() < 56.0;
            px.extend_from_slice(&[(80 + x) as u8, (60 + y) as u8, 230, if inside { 255 } else { 0 }]);
        }
    }
    Bitmap::from_rgba(w, h, px)
}

fn main() {
    let out = std::env::args().nth(1).expect("usage: sampler <out.png>");
    let mut d = Document::new(base_ui(), 1.25);
    let red = Style::default();
    let blue = Style { color: Rgba::opaque(0x25, 0x63, 0xEB), ..red };
    let green = Style { color: Rgba::opaque(0x16, 0xA3, 0x4A), ..red };
    let yellow = Style { color: Rgba::opaque(0xFA, 0xCC, 0x15), ..red };
    let purple = Style { color: Rgba::opaque(0x8B, 0x5C, 0xF6), ..red };
    let p = Pt::new;

    d.add(Shape::Spotlight { rect: RectF::new(266.0, 86.0, 1310.0, 890.0), oval: false }, Style { corner_radius: 16.0, ..red });
    d.add(Shape::Redact { rect: RectF::new(316.0, 206.0, 528.0, 40.0), mode: RedactMode::Pixelate, block: 10.0 }, red);
    d.add(Shape::Redact { rect: RectF::new(636.0, 548.0, 300.0, 34.0), mode: RedactMode::Blur, block: 6.0 }, red);
    d.add(Shape::Redact { rect: RectF::new(636.0, 610.0, 300.0, 34.0), mode: RedactMode::Erase, block: 8.0 }, red);
    d.add(Shape::Redact { rect: RectF::new(296.0, 650.0, 760.0, 130.0), mode: RedactMode::EraseText, block: 8.0 }, red);

    d.add(Shape::Rect { rect: RectF::new(300.0, 352.0, 166.0, 56.0) }, Style { width: 3.0, ..red });
    d.add(Shape::Rect { rect: RectF::new(1376.0, 10.0, 188.0, 44.0) }, Style { fill: Fill::Tint, width: 3.0, ..green });
    d.add(Shape::Rect { rect: RectF::new(940.0, 300.0, 64.0, 104.0) }, Style { fill: Fill::Solid, ..purple });
    d.add(Shape::Rect { rect: RectF::new(924.0, 112.0, 260.0, 52.0) }, Style { dash: Dash::Dashed, width: 3.0, ..blue });
    d.add(Shape::Oval { rect: RectF::new(1030.0, 518.0, 150.0, 60.0) }, Style { width: 3.0, ..red });
    d.add(Shape::Oval { rect: RectF::new(1210.0, 160.0, 120.0, 120.0) }, Style { fill: Fill::Tint, dash: Dash::Dotted, ..blue });
    d.add(Shape::Rect { rect: RectF::new(1340.0, 600.0, 190.0, 120.0) }, Style { hand_drawn: true, ..red });
    d.add(Shape::Oval { rect: RectF::new(1340.0, 760.0, 190.0, 110.0) }, Style { hand_drawn: true, fill: Fill::Tint, ..blue });

    d.add(Shape::Arrow { from: p(700.0, 860.0), to: p(560.0, 760.0), control: None, kind: ArrowKind::Straight }, red);
    d.add(Shape::Arrow { from: p(1250.0, 470.0), to: p(1110.0, 520.0), control: None, kind: ArrowKind::Curved }, Style { width: 6.0, ..blue });
    d.add(Shape::Arrow { from: p(760.0, 900.0), to: p(1000.0, 840.0), control: Some(p(900.0, 960.0)), kind: ArrowKind::Bendable }, green);
    d.add(Shape::Arrow { from: p(1060.0, 900.0), to: p(1300.0, 860.0), control: None, kind: ArrowKind::Straight }, Style { dash: Dash::Dashed, ..purple });
    d.add(Shape::Arrow { from: p(1250.0, 730.0), to: p(1080.0, 680.0), control: None, kind: ArrowKind::Curved }, Style { hand_drawn: true, ..red });
    d.add(Shape::Arrow { from: p(80.0, 520.0), to: p(150.0, 300.0), control: None, kind: ArrowKind::Straight }, Style { opacity: 0.6, width: 8.0, ..yellow });

    d.add(Shape::Line { from: p(300.0, 940.0), to: p(560.0, 940.0) }, Style { dash: Dash::Dotted, ..blue });
    d.add(Shape::Line { from: p(300.0, 900.0), to: p(560.0, 880.0) }, Style { hand_drawn: true, ..green });
    let wave: Vec<Pt> = (0..80).map(|i| p(960.0 + i as f32 * 6.0, 250.0 + (i as f32 * 0.25).sin() * 26.0)).collect();
    d.add(Shape::Freehand { points: wave }, Style { width: 3.0, ..purple });
    let swipe: Vec<Pt> = (0..60).map(|i| p(354.0 + i as f32 * 4.0, 566.0 + (i as f32 * 0.1).sin() * 2.0)).collect();
    d.add(Shape::Highlighter { points: swipe }, yellow);

    d.add(Shape::Text { origin: p(560.0, 120.0), text: "Rotate before Friday".into() }, red);
    d.add(Shape::Text { origin: p(1200.0, 300.0), text: "Peak: Thursday".into() }, Style { text_background: TextBackground::Outline, font_size: 24.0, ..yellow });
    d.add(Shape::Text { origin: p(620.0, 340.0), text: "Plain note\nacross two lines".into() }, Style { text_background: TextBackground::None, bold: false, font_size: 20.0, ..blue });
    d.add(Shape::Step { center: p(270.0, 112.0), number: 1 }, red);
    d.add(Shape::Step { center: p(910.0, 112.0), number: 2 }, blue);
    d.add(Shape::Step { center: p(270.0, 482.0), number: 12 }, yellow);

    d.add(Shape::Measure { from: p(280.0, 450.0), to: p(880.0, 450.0) }, Style { color: Rgba::opaque(0xEC, 0x48, 0x99), ..red });
    d.add(Shape::Measure { from: p(1574.0, 100.0), to: p(1574.0, 430.0) }, Style { color: Rgba::opaque(0xEC, 0x48, 0x99), ..red });
    d.add(Shape::Magnifier { source: RectF::new(1380.0, 14.0, 60.0, 36.0), callout: RectF::new(1430.0, 120.0, 120.0, 120.0) }, blue);
    d.add(Shape::Guide { axis: Axis::Horizontal, at: 470.0 }, Style { color: Rgba::opaque(0x06, 0xB6, 0xD4), ..red });
    d.add(Shape::Guide { axis: Axis::Vertical, at: 920.0 }, Style { color: Rgba::opaque(0x06, 0xB6, 0xD4), ..red });
    d.add(Shape::Overlay { rect: RectF::new(60.0, 820.0, 120.0, 120.0), image: logo(), opacity: 0.85 }, red);

    d.backdrop = Some(Backdrop {
        from: Rgba::opaque(0x63, 0x66, 0xF1),
        to: Rgba::opaque(0xEC, 0x48, 0x99),
        padding: 72.0,
        shadow: 36.0,
        radius: 14.0,
        aspect: None,
    });
    let img = export(&d);
    let mut rgba = img.to_rgba();
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        for c in &mut px[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
    let size = tiny_skia::IntSize::from_wh(img.width(), img.height()).unwrap();
    tiny_skia::Pixmap::from_vec(rgba, size).unwrap().save_png(&out).expect("write png");
    println!("{}x{} -> {out}", img.width(), img.height());
}
