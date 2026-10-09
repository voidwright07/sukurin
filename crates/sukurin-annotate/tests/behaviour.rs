use sukurin_annotate::*;
use sukurin_core::{Bitmap, Rgba};

const WHITE: Rgba = Rgba::opaque(255, 255, 255);
const RED: Rgba = Rgba::opaque(0xF0, 0x44, 0x3A);

fn flat(w: u32, h: u32, c: Rgba) -> Bitmap {
    Bitmap::from_rgba(w, h, [c.r, c.g, c.b, c.a].repeat((w * h) as usize))
}

fn px(b: &Bitmap, x: u32, y: u32) -> Rgba {
    b.pixel(x, y).unwrap()
}

fn close(a: Rgba, b: Rgba, tol: u8) -> bool {
    a.r.abs_diff(b.r) <= tol && a.g.abs_diff(b.g) <= tol && a.b.abs_diff(b.b) <= tol && a.a.abs_diff(b.a) <= tol
}

fn red(width: f32) -> Style {
    Style { color: RED, width, ..Style::default() }
}

fn render(shape: Shape, style: Style) -> Bitmap {
    let mut d = Document::new(flat(400, 200, WHITE), 1.0);
    d.add(shape, style);
    export(&d)
}

#[test]
fn rect_stroke_colours_its_edge_and_leaves_the_interior() {
    let out = render(Shape::Rect { rect: RectF::new(50.0, 50.0, 100.0, 80.0) }, red(4.0));
    assert_eq!(px(&out, 50, 90), RED);
    assert_eq!(px(&out, 100, 90), WHITE);
    assert_eq!(px(&out, 30, 30), WHITE);
}

#[test]
fn solid_fill_colours_the_interior() {
    let out = render(Shape::Rect { rect: RectF::new(50.0, 50.0, 100.0, 80.0) }, Style { fill: Fill::Solid, ..red(4.0) });
    assert_eq!(px(&out, 100, 90), RED);
    let tint = render(Shape::Oval { rect: RectF::new(50.0, 50.0, 100.0, 80.0) }, Style { fill: Fill::Tint, ..red(4.0) });
    let inside = px(&tint, 100, 90);
    assert!(inside != WHITE && inside != RED, "tint is a light wash, got {inside:?}");
}

#[test]
fn dashed_line_has_gaps_and_solid_does_not() {
    let line = Shape::Line { from: Pt::new(20.0, 100.0), to: Pt::new(380.0, 100.0) };
    let dashed = render(line.clone(), Style { dash: Dash::Dashed, ..red(4.0) });
    let solid = render(line, red(4.0));
    let reds = |b: &Bitmap| (30..370).filter(|&x| px(b, x, 100) == RED).count();
    let whites = |b: &Bitmap| (30..370).filter(|&x| px(b, x, 100) == WHITE).count();
    assert!(reds(&dashed) > 100 && whites(&dashed) > 60, "{} red, {} white", reds(&dashed), whites(&dashed));
    assert_eq!(reds(&solid), 340);
}

#[test]
fn arrow_head_reaches_the_tip_and_is_wider_than_the_shaft() {
    let arrow = Shape::Arrow { from: Pt::new(20.0, 100.0), to: Pt::new(300.0, 100.0), control: None, kind: ArrowKind::Straight };
    let out = render(arrow, red(4.0));
    assert_eq!(px(&out, 295, 100), RED);
    assert_eq!(px(&out, 282, 107), RED, "head flares beyond the 4px shaft");
    assert_eq!(px(&out, 150, 107), WHITE);
    assert_eq!(px(&out, 304, 100), WHITE);
}

#[test]
fn curved_arrow_bows_off_the_straight_line() {
    let arrow = Shape::Arrow { from: Pt::new(40.0, 150.0), to: Pt::new(360.0, 150.0), control: None, kind: ArrowKind::Curved };
    let out = render(arrow, red(4.0));
    assert_eq!(px(&out, 200, 150), WHITE);
    assert!((60..150).any(|y| px(&out, 200, y) == RED), "the curve passes above the chord");
}

#[test]
fn translucent_arrow_does_not_darken_where_shaft_meets_head() {
    let arrow = Shape::Arrow { from: Pt::new(20.0, 100.0), to: Pt::new(300.0, 100.0), control: None, kind: ArrowKind::Straight };
    let out = render(arrow, Style { opacity: 0.5, ..red(6.0) });
    let shaft = px(&out, 150, 100);
    for x in 150..296 {
        assert!(close(px(&out, x, 100), shaft, 2), "x={x}: {:?} vs {shaft:?}", px(&out, x, 100));
    }
}

#[test]
fn text_marks_pixels_inside_its_bounds_only() {
    let mut d = Document::new(flat(400, 200, WHITE), 1.0);
    let style = Style { color: Rgba::opaque(0, 0, 0), font_size: 40.0, text_background: TextBackground::None, ..Style::default() };
    let id = d.add(Shape::Text { origin: Pt::new(50.0, 50.0), text: "Hi".into() }, style);
    let b = d.bounds(id).unwrap();
    let out = export(&d);
    let mut inside = 0;
    for y in 0..200 {
        for x in 0..400 {
            let changed = px(&out, x, y) != WHITE;
            let p = Pt::new(x as f32 + 0.5, y as f32 + 0.5);
            if RectF::new(b.x - 1.0, b.y - 1.0, b.w + 2.0, b.h + 2.0).contains(p) {
                inside += changed as u32;
            } else {
                assert!(!changed, "pixel ({x},{y}) outside {b:?} changed");
            }
        }
    }
    assert!(inside > 150, "only {inside} glyph pixels");
    assert!(b.w > 30.0 && b.w < 80.0 && b.h > 35.0 && b.h < 60.0, "{b:?}");
}

#[test]
fn step_shows_its_number_inside_the_circle() {
    let out = render(Shape::Step { center: Pt::new(100.0, 100.0), number: 7 }, Style { font_size: 40.0, ..red(4.0) });
    let (mut reds, mut whites) = (0, 0);
    for y in 85..115 {
        for x in 85..115 {
            let p = px(&out, x, y);
            reds += (p == RED) as u32;
            whites += (p == WHITE) as u32;
        }
    }
    assert!(reds > 200 && whites > 20, "{reds} red, {whites} white");
}

#[test]
fn spotlight_darkens_outside_the_hole_only() {
    let grey = Rgba::opaque(128, 128, 128);
    let mut d = Document::new(flat(200, 200, grey), 1.0);
    d.add(Shape::Spotlight { rect: RectF::new(50.0, 50.0, 100.0, 100.0), oval: false }, Style::default());
    let out = export(&d);
    assert_eq!(px(&out, 100, 100), grey);
    let dark = px(&out, 10, 10);
    assert!(dark.r < 60 && dark.r > 40, "{dark:?}");
    let oval = {
        let mut d = Document::new(flat(200, 200, grey), 1.0);
        d.add(Shape::Spotlight { rect: RectF::new(50.0, 50.0, 100.0, 100.0), oval: true }, Style::default());
        export(&d)
    };
    assert_eq!(px(&oval, 100, 100), grey);
    assert!(px(&oval, 55, 55).r < 60, "oval hole leaves its corners dark");
}

fn noise(w: u32, h: u32) -> Bitmap {
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            v.extend_from_slice(&[(x * 7 + y * 13) as u8, (x * x + y) as u8, (y * 5) as u8, 255]);
        }
    }
    Bitmap::from_rgba(w, h, v)
}

#[test]
fn pixelate_makes_uniform_blocks() {
    let mut d = Document::new(noise(200, 150), 1.0);
    d.add(Shape::Redact { rect: RectF::new(20.0, 20.0, 80.0, 60.0), mode: RedactMode::Pixelate, block: 10.0 }, Style::default());
    let out = export(&d);
    for by in 0..6 {
        for bx in 0..8 {
            let (x0, y0) = (20 + bx * 10, 20 + by * 10);
            let first = px(&out, x0, y0);
            for y in y0..y0 + 10 {
                for x in x0..x0 + 10 {
                    assert_eq!(px(&out, x, y), first, "block ({bx},{by}) at ({x},{y})");
                }
            }
        }
    }
    assert_ne!(px(&out, 20, 20), px(&out, 90, 70));
    assert_eq!(px(&out, 10, 10), px(&noise(200, 150), 10, 10));
}

#[test]
fn blur_smooths_the_region() {
    let src = noise(200, 150);
    let mut d = Document::new(src.clone(), 1.0);
    d.add(Shape::Redact { rect: RectF::new(20.0, 20.0, 80.0, 60.0), mode: RedactMode::Blur, block: 8.0 }, Style::default());
    let out = export(&d);
    let jumps = |b: &Bitmap| (30..90).map(|x| px(b, x, 50).g.abs_diff(px(b, x + 1, 50).g) as u32).sum::<u32>();
    assert!(jumps(&out) * 5 < jumps(&src), "{} vs {}", jumps(&out), jumps(&src));
}

#[test]
fn erase_on_a_flat_colour_removes_the_blob() {
    let bg = Rgba::opaque(200, 180, 90);
    let mut base = Document::new(flat(200, 120, bg), 1.0);
    base.add(Shape::Rect { rect: RectF::new(80.0, 50.0, 30.0, 12.0) }, Style { color: Rgba::opaque(0, 0, 0), fill: Fill::Solid, ..Style::default() });
    let mut d = Document::new(export(&base), 1.0);
    d.add(Shape::Redact { rect: RectF::new(60.0, 35.0, 70.0, 40.0), mode: RedactMode::Erase, block: 8.0 }, Style::default());
    let out = export(&d);
    for y in 35..75 {
        for x in 60..130 {
            assert!(close(px(&out, x, y), bg, 1), "({x},{y}) = {:?}", px(&out, x, y));
        }
    }
}

#[test]
fn erase_on_a_gradient_continues_the_gradient() {
    let mut v = Vec::new();
    for _y in 0..100 {
        for x in 0..200u32 {
            v.extend_from_slice(&[x as u8, 100, 200 - x as u8, 255]);
        }
    }
    let grad = Bitmap::from_rgba(200, 100, v);
    let mut d = Document::new(grad.clone(), 1.0);
    d.add(Shape::Redact { rect: RectF::new(50.0, 30.0, 100.0, 40.0), mode: RedactMode::Erase, block: 8.0 }, Style::default());
    let out = export(&d);
    for x in 52..148 {
        assert!(close(px(&out, x, 50), px(&grad, x, 50), 3), "x={x}");
    }
}

#[test]
fn erase_text_removes_text_but_keeps_the_panel_border() {
    let page = Rgba::opaque(0xF3, 0xF4, 0xF6);
    let border = Rgba::opaque(0xC8, 0xCC, 0xD2);
    let mut base = Document::new(flat(300, 160, page), 1.0);
    let solid = |c| Style { color: c, fill: Fill::Solid, width: 1.0, corner_radius: 0.0, ..Style::default() };
    base.add(Shape::Rect { rect: RectF::new(40.5, 30.5, 200.0, 90.0) }, solid(border));
    base.add(Shape::Rect { rect: RectF::new(41.5, 31.5, 198.0, 88.0) }, solid(WHITE));
    let ink = Style { color: Rgba::opaque(0x22, 0x26, 0x2E), font_size: 18.0, text_background: TextBackground::None, ..Style::default() };
    base.add(Shape::Text { origin: Pt::new(56.0, 44.0), text: "Account 4417\nsecret token".into() }, ink);
    let avatar = Rgba::opaque(0x25, 0x63, 0xEB);
    base.add(Shape::Oval { rect: RectF::new(196.0, 70.0, 32.0, 32.0) }, solid(avatar));
    let base = export(&base);
    assert_eq!(px(&base, 40, 60), border);
    assert!((44..90).any(|y| (56..200).any(|x| px(&base, x, y).r < 100)), "fixture has dark text");

    let mut d = Document::new(base, 1.0);
    d.add(Shape::Redact { rect: RectF::new(30.0, 20.0, 230.0, 115.0), mode: RedactMode::EraseText, block: 8.0 }, Style::default());
    let out = export(&d);
    for y in 36..115 {
        for x in 46..190 {
            assert!(close(px(&out, x, y), WHITE, 2), "text pixel ({x},{y}) left as {:?}", px(&out, x, y));
        }
    }
    for y in 74..98 {
        assert_eq!(px(&out, 212, y), avatar, "solid block survives at y={y}");
    }
    for y in 32..118 {
        assert_eq!(px(&out, 40, y), border, "left border at y={y}");
        assert_eq!(px(&out, 240, y), border, "right border at y={y}");
    }
    for x in 42..238 {
        assert_eq!(px(&out, x, 30), border, "top border at x={x}");
    }
    assert_eq!(px(&out, 35, 60), page);
}

#[test]
fn magnifier_shows_enlarged_source_pixels_inside_its_circle() {
    let mut base = Document::new(flat(300, 200, WHITE), 1.0);
    base.add(Shape::Rect { rect: RectF::new(20.0, 20.0, 10.0, 10.0) }, Style { color: Rgba::opaque(0, 0, 255), fill: Fill::Solid, width: 0.1, corner_radius: 0.0, ..Style::default() });
    let mut d = Document::new(export(&base), 1.0);
    d.add(Shape::Magnifier { source: RectF::new(15.0, 15.0, 20.0, 20.0), callout: RectF::new(150.0, 50.0, 100.0, 100.0) }, Style { color: RED, ..Style::default() });
    let out = export(&d);
    assert_eq!(px(&out, 200, 100), Rgba::opaque(0, 0, 255), "centre of the source is blue");
    assert_eq!(px(&out, 160, 60), WHITE, "corners outside the circle are untouched");
    assert_eq!(px(&out, 200, 50), RED, "ring");
}

#[test]
fn measure_draws_a_labelled_line() {
    let mut d = Document::new(flat(400, 200, WHITE), 2.0);
    let id = d.add(Shape::Measure { from: Pt::new(50.0, 100.0), to: Pt::new(350.0, 100.0) }, red(4.0));
    let b = d.bounds(id).unwrap();
    assert!(b.x <= 50.0 && (b.x + b.w) >= 350.0);
    let out = export(&d);
    assert_eq!(px(&out, 100, 100), RED);
    assert!((90..110).any(|y| px(&out, 200, y) == WHITE) && (90..110).any(|y| px(&out, 200, y) == RED), "label pill with white text");
}

#[test]
fn guide_spans_the_canvas_dashed() {
    let mut d = Document::new(flat(300, 100, WHITE), 1.0);
    d.add(Shape::Guide { axis: Axis::Horizontal, at: 40.0 }, red(4.0));
    let out = export(&d);
    let reds = (0..300).filter(|&x| px(&out, x, 40) == RED).count();
    assert!(reds > 120 && reds < 250, "{reds}");
    assert_eq!(px(&out, 100, 42), WHITE);
}

#[test]
fn overlay_and_opacity_blend_over_the_base() {
    let mut d = Document::new(flat(100, 100, WHITE), 1.0);
    d.add(Shape::Overlay { rect: RectF::new(10.0, 10.0, 40.0, 40.0), image: flat(4, 4, Rgba::opaque(0, 0, 0)), opacity: 0.5 }, Style::default());
    d.add(Shape::Rect { rect: RectF::new(60.0, 60.0, 30.0, 30.0) }, Style { fill: Fill::Solid, opacity: 0.5, color: Rgba::opaque(0, 0, 0), ..Style::default() });
    let out = export(&d);
    assert!(close(px(&out, 30, 30), Rgba::opaque(128, 128, 128), 2), "{:?}", px(&out, 30, 30));
    assert!(close(px(&out, 75, 75), Rgba::opaque(128, 128, 128), 2), "{:?}", px(&out, 75, 75));
}

#[test]
fn hit_test_finds_strokes_and_skips_unfilled_interiors() {
    let mut d = Document::new(flat(400, 200, WHITE), 1.0);
    let outline = d.add(Shape::Rect { rect: RectF::new(50.0, 50.0, 100.0, 80.0) }, red(4.0));
    let filled = d.add(Shape::Oval { rect: RectF::new(200.0, 50.0, 100.0, 80.0) }, Style { fill: Fill::Tint, ..red(4.0) });
    assert_eq!(d.hit_test(Pt::new(55.0, 90.0), 4.0), Some(outline), "within tolerance of the left edge");
    assert_eq!(d.hit_test(Pt::new(100.0, 90.0), 4.0), None, "empty interior");
    assert_eq!(d.hit_test(Pt::new(250.0, 90.0), 4.0), Some(filled));
    assert_eq!(d.hit_test(Pt::new(380.0, 20.0), 4.0), None);
    let top = d.add(Shape::Text { origin: Pt::new(40.0, 80.0), text: "over".into() }, Style::default());
    assert_eq!(d.hit_test(Pt::new(52.0, 90.0), 2.0), Some(top), "topmost wins");
}

#[test]
fn bounds_cover_the_stroke_and_arrowhead() {
    let mut d = Document::new(flat(400, 200, WHITE), 1.0);
    let r = d.add(Shape::Rect { rect: RectF::new(50.0, 50.0, 100.0, 80.0) }, red(4.0));
    let b = d.bounds(r).unwrap();
    assert!((b.x - 48.0).abs() < 0.5 && (b.w - 104.0).abs() < 0.5, "{b:?}");
    let a = d.add(Shape::Arrow { from: Pt::new(20.0, 100.0), to: Pt::new(300.0, 100.0), control: None, kind: ArrowKind::Straight }, red(4.0));
    let b = d.bounds(a).unwrap();
    assert!(((b.x + b.w) - 300.0).abs() < 0.5 && b.h > 18.0, "{b:?}");
    assert!(d.bounds(AnnotationId(999)).is_none());
}

#[test]
fn hand_drawn_geometry_is_stable_per_id() {
    let mut d = Document::new(flat(400, 200, WHITE), 1.0);
    let style = Style { hand_drawn: true, ..red(4.0) };
    let shapes = [
        Shape::Rect { rect: RectF::new(50.0, 50.0, 100.0, 80.0) },
        Shape::Oval { rect: RectF::new(50.0, 50.0, 100.0, 80.0) },
        Shape::Line { from: Pt::new(10.0, 10.0), to: Pt::new(300.0, 120.0) },
        Shape::Arrow { from: Pt::new(10.0, 10.0), to: Pt::new(300.0, 120.0), control: None, kind: ArrowKind::Curved },
    ];
    for shape in shapes {
        let a = d.add(shape.clone(), style);
        let b = d.add(shape, style);
        let scene = |id| format!("{:?}", annotation_scene(&d, id));
        assert_eq!(scene(a), scene(a));
        assert_ne!(scene(a), scene(b), "different ids sketch differently");
    }
}

#[test]
fn backdrop_pads_content_and_paints_the_gradient() {
    let from = Rgba::opaque(255, 0, 0);
    let to = Rgba::opaque(0, 0, 255);
    let content = Rgba::opaque(0, 200, 0);
    let mut d = Document::new(flat(100, 80, content), 1.0);
    d.backdrop = Some(Backdrop { from, to, padding: 30.0, shadow: 10.0, radius: 8.0, aspect: None });
    let out = export(&d);
    assert_eq!((out.width(), out.height()), (160, 140));
    assert!(close(px(&out, 0, 0), from, 4), "{:?}", px(&out, 0, 0));
    assert!(close(px(&out, 159, 139), to, 4), "{:?}", px(&out, 159, 139));
    assert_eq!(px(&out, 80, 70), content);
    assert_ne!(px(&out, 30, 30), content, "rounded corner masks the content");

    d.backdrop = Some(Backdrop { from, to, padding: 30.0, shadow: 0.0, radius: 0.0, aspect: Some((16, 9)) });
    let wide = export(&d);
    assert_eq!((wide.width(), wide.height()), (249, 140));
}

#[test]
fn crop_limits_the_export() {
    let mut d = Document::new(noise(200, 150), 1.0);
    d.crop = Some(RectF::new(40.0, 30.0, 50.0, 20.0));
    let out = export(&d);
    assert_eq!((out.width(), out.height()), (50, 20));
    assert_eq!(px(&out, 0, 0), px(&noise(200, 150), 40, 30));
}

#[test]
fn layers_extend_the_canvas() {
    let mut d = Document::new(flat(100, 50, WHITE), 1.0);
    d.layers.push(Layer { bitmap: flat(40, 40, RED), origin: Pt::new(100.0, 30.0) });
    assert_eq!(d.canvas_bounds(), RectF::new(0.0, 0.0, 140.0, 70.0));
    assert_eq!(layers_scene(&d).len(), 2);
    let out = export(&d);
    assert_eq!((out.width(), out.height()), (140, 70));
    assert_eq!(px(&out, 120, 50), RED);
    assert_eq!(px(&out, 50, 60).a, 0, "uncovered canvas is transparent");
}

#[test]
fn next_step_follows_the_highest_number() {
    let mut d = Document::new(flat(10, 10, WHITE), 1.0);
    assert_eq!(d.next_step(), 1);
    d.add(Shape::Step { center: Pt::new(1.0, 1.0), number: 4 }, Style::default());
    d.add(Shape::Step { center: Pt::new(1.0, 1.0), number: 2 }, Style::default());
    assert_eq!(d.next_step(), 5);
}

#[test]
fn history_undo_redo_round_trips() {
    let mut d = Document::new(flat(10, 10, WHITE), 1.0);
    let mut h = History::default();
    h.checkpoint(&d);
    let id = d.add(Shape::Line { from: Pt::new(0.0, 0.0), to: Pt::new(5.0, 5.0) }, Style::default());
    h.checkpoint(&d);
    d.get_mut(id).unwrap().style.width = 9.0;
    assert!(h.undo(&mut d));
    assert_eq!(d.get(id).unwrap().style.width, Style::default().width);
    assert!(h.undo(&mut d));
    assert!(d.annotations.is_empty());
    assert!(!h.undo(&mut d));
    assert!(h.redo(&mut d));
    assert!(h.redo(&mut d));
    assert_eq!(d.get(id).unwrap().style.width, 9.0);
    assert!(!h.redo(&mut d));
}

#[test]
fn opacity_scales_every_prim() {
    let mut d = Document::new(flat(10, 10, WHITE), 1.0);
    let id = d.add(Shape::Step { center: Pt::new(5.0, 5.0), number: 1 }, Style { opacity: 0.5, ..Style::default() });
    for prim in annotation_scene(&d, id) {
        if let Prim::Fill { color, .. } = prim {
            assert!(color.a <= 128, "{color:?}");
        }
    }
}
