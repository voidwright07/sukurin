//! Joins successive grabs of a scrolling region into one tall image.
//!
//! Each frame is compared with the previous one row by row; the scroll
//! distance is the row offset under which the moving band of the new frame
//! lines up with the old one. Rows that stay put at the top and bottom of the
//! first pair are left out of that comparison.
//!
//! A frame's rows are taken only from its middle: the margins above and
//! below are as tall as half the part the next frame does not newly show.
//! Whatever sits at the edges of the region, a sticky header, a cookie bar,
//! or a strip of another window that changes between grabs, therefore never
//! lands between two frames; the first frame keeps its top and the last
//! frame is pasted whole.

use crate::Bitmap;

pub struct Stitcher {
    width: u32,
    frame_height: u32,
    /// Page rows taken so far, top to bottom, as BGRA bytes.
    body: Vec<u8>,
    sticky: Option<(u32, u32)>,
    /// Page row shown on the first row of the last frame.
    pos: usize,
    last: Option<Frame>,
    frames: u32,
    /// The last accepted scroll distance and the frames lost since it.
    step: Option<usize>,
    lost: usize,
}

struct Frame {
    bitmap: Bitmap,
    rows: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Push {
    /// The content moved by this many rows; they were appended.
    Added(u32),
    /// Nothing moved: the end of the page, or the content is still loading.
    Unchanged,
    /// The new frame could not be aligned with the previous one (the user
    /// scrolled too far in one step, or the content reflowed). Skipped.
    Lost,
}

impl Stitcher {
    pub fn new() -> Self {
        Self { width: 0, frame_height: 0, body: Vec::new(), sticky: None, pos: 0, last: None, frames: 0, step: None, lost: 0 }
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// Height of the image `finish` would produce right now.
    pub fn height(&self) -> u32 {
        if self.last.is_none() {
            return 0;
        }
        (self.pos + self.frame_height as usize) as u32
    }

    fn taken(&self) -> usize {
        self.body.len() / (self.width as usize * 4)
    }

    pub fn push(&mut self, b: &Bitmap) -> Push {
        let rows = row_hashes(b);
        let Some(prev) = self.last.take() else {
            self.width = b.width();
            self.frame_height = b.height();
            self.last = Some(Frame { bitmap: b.clone(), rows });
            self.frames = 1;
            return Push::Added(b.height());
        };
        if b.width() != self.width || b.height() != self.frame_height {
            self.last = Some(prev);
            self.lost += 1;
            return Push::Lost;
        }
        if rows == prev.rows {
            self.last = Some(prev);
            return Push::Unchanged;
        }
        let h = rows.len();
        let top = rows.iter().zip(&prev.rows).take_while(|(a, b)| a == b).count();
        let bottom = rows.iter().rev().zip(prev.rows.iter().rev()).take_while(|(a, b)| a == b).count();
        let (top, bottom) = match self.sticky {
            Some((t, b)) => (t as usize, b as usize),
            None => (top.min(h / 3), bottom.min(h / 3)),
        };
        let prior = self.step.map(|step| (step, self.lost));
        let Some(shift) = best_shift(&prev.rows[top..h - bottom], &rows[top..h - bottom], prior) else {
            self.last = Some(prev);
            self.lost += 1;
            return Push::Lost;
        };
        self.step = Some(shift);
        self.lost = 0;
        self.sticky.get_or_insert((top as u32, bottom as u32));
        let row = self.width as usize * 4;
        let from = self.taken() - self.pos;
        let to = shift + (h - shift) / 2;
        if from < to {
            self.body.extend_from_slice(&prev.bitmap.bgra()[from * row..to * row]);
        }
        self.pos += shift;
        self.last = Some(Frame { bitmap: b.clone(), rows });
        self.frames += 1;
        Push::Added(shift as u32)
    }

    pub fn finish(&self) -> Option<Bitmap> {
        let last = self.last.as_ref()?;
        let row = self.width as usize * 4;
        let mut out = self.body.clone();
        out.extend_from_slice(&last.bitmap.bgra()[(self.taken() - self.pos) * row..]);
        Some(Bitmap::from_bgra(self.width, self.height(), out))
    }
}

impl Default for Stitcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Hashes the middle 70% of each row. The edges of a region hold what does
/// not scroll with the page (a window border, the scrollbar thumb), which
/// would make every row differ under the true shift.
fn row_hashes(b: &Bitmap) -> Vec<u64> {
    let w = b.width() as usize;
    let margin = w * 15 / 100;
    b.bgra()
        .chunks_exact(w * 4)
        .map(|r| {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for chunk in r[margin * 4..(w - margin) * 4].as_chunks::<4>().0 {
                h ^= u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]) as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            h
        })
        .collect()
}

/// The scroll distance `s` (rows the content moved up) for which
/// `new[y] == old[y + s]` over the overlap.
///
/// Real screens are not bit-exact between frames (an HDR desktop converted
/// to 8-bit is dithered, so a few pixels per row wobble by one level), so
/// a perfect match is not required. Instead the winning shift must match
/// most rows and, before any shift has been accepted, beat every other
/// shift by a clear margin; on a flat background all shifts tie, which
/// correctly yields no alignment.
///
/// Once a shift has been accepted, `prior` is `(step, lost)`: the page has
/// been scrolling by `step` and `lost` frames have gone unaligned since, so
/// the next frame is expected at `step * (lost + 1)`. Repeating content (a
/// striped table) makes shifts a period apart tie, so among the shifts that
/// match about as well as the best one the closest to the expectation wins,
/// with no margin test. Shifts well past the expectation are not scored.
fn best_shift(old: &[u64], new: &[u64], prior: Option<(usize, usize)>) -> Option<usize> {
    const MIN_MATCH: f32 = 0.7;
    const MIN_MARGIN: f32 = 0.2;
    const TIE: f32 = 0.02;
    let h = old.len();
    let informative = (0..h.saturating_sub(1)).filter(|&y| new[y] != new[y + 1]).count();
    if informative < 4 {
        return None;
    }
    let max = h.saturating_sub((h / 5).max(8));
    let ratio = |s: usize| {
        let overlap = h - s;
        (0..overlap).filter(|&y| new[y] == old[y + s]).count() as f32 / overlap as f32
    };
    let s = match prior {
        None => {
            let (mut best, mut runner_up) = ((0usize, 0f32), 0f32);
            for s in 1..=max {
                let r = ratio(s);
                if r > best.1 {
                    runner_up = best.1;
                    best = (s, r);
                } else if r > runner_up {
                    runner_up = r;
                }
            }
            if best.1 < MIN_MATCH || best.1 - runner_up < MIN_MARGIN {
                return None;
            }
            best.0
        }
        Some((step, lost)) => {
            let expected = step * (lost + 1);
            let scored: Vec<(usize, f32)> = (1..=max.min(expected + step / 2)).map(|s| (s, ratio(s))).collect();
            let best = scored.iter().map(|&(_, r)| r).fold(0f32, f32::max);
            if best < MIN_MATCH {
                return None;
            }
            scored.iter().filter(|&&(_, r)| r >= best - TIE).min_by_key(|&&(s, _)| s.abs_diff(expected))?.0
        }
    };
    (!torn(old, new, s)).then_some(s)
}

/// Dither noise mismatches scattered single rows; a frame grabbed while
/// the page was half repainted mismatches one solid block. Accepting such a
/// frame would append stale rows, so it is rejected and the next, settled
/// frame aligns instead.
fn torn(old: &[u64], new: &[u64], s: usize) -> bool {
    let overlap = old.len() - s;
    let mut run = 0;
    let mut longest = 0;
    for y in 0..overlap {
        if new[y] == old[y + s] {
            run = 0;
        } else {
            run += 1;
            longest = longest.max(run);
        }
    }
    longest * 20 > overlap.max(40)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tall "page" whose rows each have a unique pattern, viewed through a
    /// window of `view` rows with a sticky header and footer painted on top.
    fn page(height: u32, _width: u32) -> Vec<[u8; 4]> {
        (0..height).map(|y| [(y * 37 % 251) as u8, (y * 11 % 241) as u8, (y / 7) as u8, 255]).collect()
    }

    fn view(page: &[[u8; 4]], width: u32, scroll: u32, view: u32, header: u32, footer: u32) -> Bitmap {
        let mut v = Vec::new();
        for y in 0..view {
            let c = if y < header {
                [10, 20, 30 + y as u8, 255]
            } else if y >= view - footer {
                [200, 210, 220 - (view - y) as u8, 255]
            } else {
                page[(scroll + y) as usize]
            };
            for x in 0..width {
                let mut p = c;
                p[0] = p[0].wrapping_add((x % 3) as u8);
                v.extend_from_slice(&p);
            }
        }
        Bitmap::from_bgra(width, view, v)
    }

    #[test]
    fn stitches_scrolled_frames_into_the_full_page_with_sticky_chrome_once() {
        let (w, total, vh, header, footer) = (16, 500, 120, 10, 6);
        let p = page(total, w);
        let mut s = Stitcher::new();
        let mut scroll = 0;
        while scroll + vh - footer <= total {
            let frame = view(&p, w, scroll, vh, header, footer);
            assert!(matches!(s.push(&frame), Push::Added(_)), "frame at {scroll} aligned");
            scroll += 37;
        }
        let out = s.finish().unwrap();
        let body_rows = out.height() - header - footer;
        let expected = view(&p, w, 0, vh, header, footer);
        assert_eq!(out.pixel(0, 0), expected.pixel(0, 0), "header kept");
        for y in 0..body_rows {
            let want = p[(header + y) as usize];
            let got = out.pixel(0, header + y).unwrap();
            assert_eq!((got.b, got.g), (want[0], want[1]), "page row {y}");
        }
        assert_eq!(out.pixel(0, out.height() - 1).unwrap().r, 220 - 1, "footer kept at the bottom");
    }

    /// Frames from a dithered HDR desktop: a few pixels per row are off by
    /// one level, differently in every frame.
    fn dither(b: &Bitmap, seed: u32) -> Bitmap {
        let mut v = b.bgra().to_vec();
        let row = b.width() as usize * 4;
        for y in 0..b.height() as usize {
            if (y as u32 * 7 + seed).is_multiple_of(9) {
                let x = ((y as u32 * 13 + seed * 5) % b.width()) as usize;
                v[y * row + x * 4] ^= 1;
            }
        }
        Bitmap::from_bgra(b.width(), b.height(), v)
    }

    #[test]
    fn aligns_frames_whose_pixels_wobble_by_one_level() {
        let (w, vh) = (16, 120);
        let p = page(600, w);
        let mut s = Stitcher::new();
        let mut scroll = 0;
        let mut seed = 0;
        while scroll + vh <= 600 {
            let frame = dither(&view(&p, w, scroll, vh, 0, 0), seed);
            assert!(matches!(s.push(&frame), Push::Added(_)), "dithered frame at {scroll} aligned");
            scroll += 41;
            seed += 3;
        }
        assert!(s.height() >= scroll - 41 + vh);
    }

    #[test]
    fn a_half_repainted_frame_is_skipped_rather_than_stitched() {
        let (w, vh) = (16, 200);
        let p = page(800, w);
        let mut s = Stitcher::new();
        s.push(&view(&p, w, 0, vh, 0, 0));
        s.push(&view(&p, w, 30, vh, 0, 0));
        let fresh = view(&p, w, 90, vh, 0, 0);
        let stale = view(&p, w, 30, vh, 0, 0);
        let row = w as usize * 4;
        let mut torn = fresh.bgra()[..row * 120].to_vec();
        torn.extend_from_slice(&stale.bgra()[row * 120..]);
        assert_eq!(s.push(&Bitmap::from_bgra(w, vh, torn)), Push::Lost);
        assert_eq!(s.push(&fresh), Push::Added(60));
        let out = s.finish().unwrap();
        for y in 0..out.height() {
            let want = p[y as usize];
            assert_eq!(out.pixel(0, y).unwrap().b, want[0], "row {y} follows the page with no repeats");
        }
    }

    #[test]
    fn an_unmoved_frame_is_reported_unchanged() {
        let p = page(300, 8);
        let mut s = Stitcher::new();
        s.push(&view(&p, 8, 0, 100, 0, 0));
        assert_eq!(s.push(&view(&p, 8, 0, 100, 0, 0)), Push::Unchanged);
        assert_eq!(s.height(), 100);
    }

    #[test]
    fn a_jump_with_no_overlap_is_lost_and_skipped() {
        let p = page(1000, 8);
        let mut s = Stitcher::new();
        s.push(&view(&p, 8, 0, 100, 0, 0));
        assert_eq!(s.push(&view(&p, 8, 500, 100, 0, 0)), Push::Lost);
        assert!(matches!(s.push(&view(&p, 8, 40, 100, 0, 0)), Push::Added(40)));
    }

    /// A region that also covers what does not scroll with the page: a
    /// one-pixel window border whose shade drifts down its length, and a
    /// scrollbar whose thumb moves a little with every step.
    fn framed(page: &[[u8; 4]], width: u32, scroll: u32, view: u32, total: u32) -> Bitmap {
        let mut v = Vec::new();
        let thumb = view * view / total;
        let thumb_top = scroll * view / total;
        for y in 0..view {
            for x in 0..width {
                let p = if x == 0 || x == width - 1 {
                    [60 + (y / 40) as u8, 60 + (y / 40) as u8, 60 + (y / 40) as u8, 255]
                } else if x >= width - 4 {
                    if (thumb_top..thumb_top + thumb).contains(&y) { [160, 160, 160, 255] } else { [44, 44, 44, 255] }
                } else {
                    page[(scroll + y) as usize]
                };
                v.extend_from_slice(&p);
            }
        }
        Bitmap::from_bgra(width, view, v)
    }

    #[test]
    fn a_window_border_and_scrollbar_in_the_region_do_not_break_alignment() {
        let (w, vh, total) = (40, 200, 1200);
        let p = page(total, w);
        let mut s = Stitcher::new();
        let mut scroll = 0;
        while scroll + vh <= total {
            assert!(matches!(s.push(&framed(&p, w, scroll, vh, total)), Push::Added(_)), "frame at {scroll} aligned");
            scroll += 60;
        }
        assert_eq!(s.height(), scroll - 60 + vh);
    }

    /// A page with unique rows, then a long run that repeats every `period`
    /// rows, like a striped table whose rows hold the same text.
    fn periodic_page(height: u32, unique: u32, period: u32) -> Vec<[u8; 4]> {
        let p = page(height, 0);
        (0..height).map(|y| if y < unique { p[y as usize] } else { p[(unique + (y - unique) % period) as usize] }).collect()
    }

    #[test]
    fn repeating_content_keeps_the_step_the_page_has_been_scrolling_by() {
        let (w, vh, step, period) = (16, 200, 60, 14);
        let p = periodic_page(1400, 300, period);
        let mut s = Stitcher::new();
        let mut scroll = 0;
        while scroll + vh <= 1400 {
            assert_eq!(s.push(&view(&p, w, scroll, vh, 0, 0)), if scroll == 0 { Push::Added(vh) } else { Push::Added(step) }, "frame at {scroll}");
            scroll += step;
        }
    }

    #[test]
    fn a_changing_strip_below_the_page_appears_once_at_the_bottom() {
        let (w, vh, strip, step) = (16, 400, 10, 37);
        let p = page(1000, w);
        let mut s = Stitcher::new();
        let mut scroll = 0;
        let mut n = 0u8;
        while scroll + vh <= 1000 {
            let mut v = view(&p, w, scroll, vh, 0, 0).bgra().to_vec();
            for px in v[((vh - strip) * w * 4) as usize..].as_chunks_mut::<4>().0 {
                px.copy_from_slice(&[20, 20, 20u8.wrapping_add(n), 255]);
            }
            assert!(matches!(s.push(&Bitmap::from_bgra(w, vh, v)), Push::Added(_)), "frame at {scroll} aligned");
            scroll += step;
            n += 1;
        }
        let out = s.finish().unwrap();
        assert_eq!(out.height(), scroll - step + vh, "every page row once, plus the strip once");
        for y in 0..out.height() - strip {
            assert_eq!(out.pixel(0, y).unwrap().b, p[y as usize][0], "row {y} is the page, not the strip");
        }
    }

    #[test]
    fn flat_background_does_not_fake_an_alignment() {
        let flat = Bitmap::from_bgra(8, 100, vec![240u8; 8 * 100 * 4]);
        let mut other = vec![240u8; 8 * 100 * 4];
        other[50 * 32 + 4 * 4] = 0;
        let mut s = Stitcher::new();
        s.push(&flat);
        assert_eq!(s.push(&Bitmap::from_bgra(8, 100, other)), Push::Lost);
    }
}
