//! Run with: cargo test -p shattr-annotate --release --test perf -- --ignored --nocapture

use std::time::{Duration, Instant};

use shattr_annotate::*;
use shattr_core::{Bitmap, Rgba};

fn base(w: u32, h: u32) -> Bitmap {
    let mut v = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            v.extend_from_slice(&[(x / 7) as u8, (y / 5) as u8, ((x + y) / 9) as u8, 255]);
        }
    }
    Bitmap::from_rgba(w, h, v)
}

/// Every vector kind, cycling through styles, spread over the canvas.
fn mixed(doc: &mut Document, n: usize) {
    for i in 0..n {
        let f = i as f32;
        let (x, y) = (60.0 + (f * 211.0) % 2300.0, 60.0 + (f * 137.0) % 1400.0);
        let style = Style {
            color: Rgba::opaque((i * 40) as u8, 120, 200),
            dash: [Dash::Solid, Dash::Dashed, Dash::Dotted][i % 3],
            fill: [Fill::None, Fill::Tint, Fill::Solid][i % 3],
            hand_drawn: i % 4 == 0,
            opacity: if i % 5 == 0 { 0.7 } else { 1.0 },
            ..Style::default()
        };
        let shape = match i % 10 {
            0 => Shape::Arrow { from: Pt::new(x, y), to: Pt::new(x + 180.0, y + 60.0), control: None, kind: ArrowKind::Curved },
            1 => Shape::Rect { rect: RectF::new(x, y, 160.0, 90.0) },
            2 => Shape::Oval { rect: RectF::new(x, y, 140.0, 90.0) },
            3 => Shape::Text { origin: Pt::new(x, y), text: format!("Note {i}: check this") },
            4 => Shape::Step { center: Pt::new(x, y), number: i as u32 },
            5 => Shape::Freehand { points: (0..120).map(|k| Pt::new(x + k as f32 * 2.0, y + (k as f32 * 0.2).sin() * 20.0)).collect() },
            6 => Shape::Highlighter { points: (0..60).map(|k| Pt::new(x + k as f32 * 3.0, y)).collect() },
            7 => Shape::Measure { from: Pt::new(x, y), to: Pt::new(x + 200.0, y + 3.0) },
            8 => Shape::Line { from: Pt::new(x, y), to: Pt::new(x + 150.0, y + 150.0) },
            _ => Shape::Arrow { from: Pt::new(x, y), to: Pt::new(x + 120.0, y - 80.0), control: None, kind: ArrowKind::Straight },
        };
        doc.add(shape, style);
    }
}

fn median(mut f: impl FnMut(), runs: usize) -> Duration {
    f();
    let mut times: Vec<Duration> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .collect();
    times.sort();
    times[runs / 2]
}

#[test]
#[ignore]
fn scene_for_sixty_annotations_is_fast() {
    let mut doc = Document::new(base(2560, 1600), 1.5);
    mixed(&mut doc, 60);
    let mut prims = 0;
    let t = median(|| prims = annotations_scene(&doc).len(), 51);
    println!("annotations_scene, 60 annotations ({prims} prims): {t:?} median");
    assert!(t < Duration::from_millis(3), "{t:?}");
}

#[test]
#[ignore]
fn export_of_a_large_document_is_fast() {
    let mut doc = Document::new(base(2560, 1600), 1.5);
    mixed(&mut doc, 20);
    let t = median(
        || {
            std::hint::black_box(export(&doc));
        },
        9,
    );
    println!("export 2560x1600, 20 annotations: {t:?} median");
    assert!(t < Duration::from_millis(120), "{t:?}");
}

#[test]
#[ignore]
fn redactions_stay_interactive() {
    for mode in [RedactMode::Pixelate, RedactMode::Blur, RedactMode::Erase, RedactMode::EraseText] {
        let mut doc = Document::new(base(2560, 1600), 1.5);
        doc.add(Shape::Redact { rect: RectF::new(300.0, 200.0, 800.0, 400.0), mode, block: 10.0 }, Style::default());
        let t = median(
            || {
                std::hint::black_box(annotations_scene(&doc));
            },
            9,
        );
        println!("{mode:?} 800x400: {t:?} median");
        assert!(t < Duration::from_millis(50), "{mode:?} {t:?}");
    }
}
