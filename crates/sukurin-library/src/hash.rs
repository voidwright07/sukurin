use std::sync::Arc;

use sukurin_core::Bitmap;

/// Grid columns. Screenshots change mostly along text lines, so columns get
/// twice the resolution of rows: a reflowed table row moves ink between
/// narrow cells instead of being averaged away.
const GW: usize = 64;
const GH: usize = 32;
const CELLS: usize = GW * GH;
/// Per grid row: brightest cell, darkest cell, distance to the pivot row.
const SUMMARY: usize = GH * 3;

/// Moving content by one source pixel relative to the grid (a crop, or a
/// resampling filter's smear) changes a cell by up to about this many levels
/// divided by the cell size in pixels. On the synthetic screenshots in the
/// tests every crop/rescale/re-encode still matched down to 140, and every
/// rewritten, added or swapped table row was still rejected up to 215; 170
/// leaves about 1.2x headroom on both sides.
const EDGE_NOISE: f32 = 170.0;
/// Headroom for JPEG noise that survives cell averaging.
const BASE_TOLERANCE: f32 = 1.5;
/// Width/height ratios further apart than this never match.
pub(crate) const MAX_ASPECT_DELTA: f32 = 0.03;
/// Below this many source pixels per cell a one-pixel shift moves most of a
/// cell, so no tolerance separates "rescaled copy" from "different image";
/// such small images (under about 192x96) only match exact copies.
const MIN_CELL_PX: f32 = 3.0;
/// Grids whose cells span at most this many levels carry too little
/// structure to tell screenshots apart, so they only match exact copies.
const FLAT_SPREAD: u8 = 6;

/// A 64x32 grid of mean luminance over exact (fractional) cell areas.
///
/// Not a Hamming-distance bit hash: on screenshots large areas are uniform,
/// so gradient bits between equal cells flip on re-encoding noise while a
/// changed table row flips only a few bits. Comparing cell means against a
/// size-aware tolerance keeps those two cases apart.
#[derive(Clone, PartialEq, Eq)]
pub struct PerceptualHash(Arc<[u8; CELLS]>);

impl std::fmt::Debug for PerceptualHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PerceptualHash({GW}x{GH})")
    }
}

impl PerceptualHash {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0[..]
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Some(Self::from_cells(bytes.try_into().ok()?))
    }

    fn from_cells(cells: [u8; CELLS]) -> Self {
        Self(Arc::new(cells))
    }

    fn rows(&self) -> std::slice::Iter<'_, [u8; GW]> {
        self.0.as_chunks::<GW>().0.iter()
    }
}

pub fn perceptual_hash(b: &Bitmap) -> PerceptualHash {
    if b.width() == 0 || b.height() == 0 {
        return PerceptualHash::from_cells([0; CELLS]);
    }
    if (b.width() as usize) < GW || (b.height() as usize) < GH {
        return perceptual_hash(&upscale_nearest(b));
    }
    let (w, h) = (b.width() as usize, b.height() as usize);
    let cols: [Span; GW] = std::array::from_fn(|c| Span::new(c, w, GW));
    let rows: [Span; GH] = std::array::from_fn(|c| Span::new(c, h, GH));
    let stride = w * 4;
    let bgra = b.bgra();
    let row_cells = |y: usize| -> [u64; GW] {
        let row = &bgra[y * stride..(y + 1) * stride];
        std::array::from_fn(|c| cols[c].weighted(GW as u64, |x0, x1| luma_sum(&row[x0 * 4..x1 * 4])))
    };
    let mut cells = [0u8; CELLS];
    let norm = (w * h * 256) as u64;
    for (cy, span) in rows.iter().enumerate() {
        let mut acc = [0u64; GW];
        let mut add = |y: usize, weight: u64| {
            for (a, v) in acc.iter_mut().zip(row_cells(y)) {
                *a += v * weight;
            }
        };
        for y in span.full.clone() {
            add(y, GH as u64);
        }
        for (y, weight) in span.left.into_iter().chain(span.right) {
            add(y, weight);
        }
        for (cell, a) in cells[cy * GW..(cy + 1) * GW].iter_mut().zip(acc) {
            *cell = ((a + norm / 2) / norm) as u8;
        }
    }
    PerceptualHash::from_cells(cells)
}

/// One grid cell's extent along an axis, measured in units of 1/grid pixels
/// so boundaries that fall inside a pixel split it exactly.
#[derive(Clone)]
struct Span {
    full: std::ops::Range<usize>,
    left: Option<(usize, u64)>,
    right: Option<(usize, u64)>,
}

impl Span {
    /// Requires `len >= grid`, so every cell is at least one pixel wide.
    fn new(c: usize, len: usize, grid: usize) -> Self {
        let (start, end) = (c * len, (c + 1) * len);
        Self {
            full: start.div_ceil(grid)..end / grid,
            left: (start % grid != 0).then(|| (start / grid, (grid - start % grid) as u64)),
            right: (end % grid != 0).then(|| (end / grid, (end % grid) as u64)),
        }
    }

    /// Sum over the span with full pixels weighted `full_weight`, given the
    /// plain sum over a pixel range.
    fn weighted(&self, full_weight: u64, sum: impl Fn(usize, usize) -> u32) -> u64 {
        let mut total = sum(self.full.start, self.full.end) as u64 * full_weight;
        for (x, weight) in self.left.into_iter().chain(self.right) {
            total += sum(x, x + 1) as u64 * weight;
        }
        total
    }
}

/// Sum of Rec.601 luma scaled by 256 over BGRA pixels. Adds two pixels per
/// u64 in 16-bit lanes, which the compiler vectorizes; the weights are
/// applied once per block. One cell's row segment stays far below u32
/// overflow (65,000 pixels).
fn luma_sum(px: &[u8]) -> u32 {
    const LANES: u64 = 0x00FF_00FF_00FF_00FF;
    let lane = |v: u64, i: u32| ((v >> (16 * i)) & 0xFFFF) as u32;
    let mut total = 0;
    for block in px.chunks(8 * 256) {
        let (words, rest) = block.as_chunks::<8>();
        let (mut blue_red, mut green) = (0u64, 0u64);
        for w in words {
            let v = u64::from_le_bytes(*w);
            blue_red += v & LANES;
            green += (v >> 8) & LANES;
        }
        let (mut b, mut g, mut r) =
            (lane(blue_red, 0) + lane(blue_red, 2), lane(green, 0) + lane(green, 2), lane(blue_red, 1) + lane(blue_red, 3));
        if let [pb, pg, pr, _] = rest {
            b += *pb as u32;
            g += *pg as u32;
            r += *pr as u32;
        }
        total += b * 29 + g * 150 + r * 77;
    }
    total
}

fn upscale_nearest(b: &Bitmap) -> Bitmap {
    let (w, h) = (b.width().max(GW as u32), b.height().max(GH as u32));
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let p = b.pixel(x * b.width() / w, y * b.height() / h).unwrap();
            out.extend_from_slice(&[p.b, p.g, p.r, p.a]);
        }
    }
    Bitmap::from_bgra(w, h, out)
}

/// Hash of the colour bytes, alpha ignored, so a capture with undefined alpha
/// still equals its decoded PNG.
pub(crate) fn content_checksum(b: &Bitmap) -> u64 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    const NO_ALPHA: u64 = 0x00FF_FFFF_00FF_FFFF;
    let mut lanes = [1u64, 2, 3, 4];
    let (chunks, rest) = b.bgra().as_chunks::<32>();
    for chunk in chunks {
        for (lane, word) in lanes.iter_mut().zip(chunk.as_chunks::<8>().0) {
            let w = u64::from_le_bytes(*word) & NO_ALPHA;
            *lane = (*lane ^ w).wrapping_mul(K).rotate_left(31);
        }
    }
    for (i, px) in rest.as_chunks::<4>().0.iter().enumerate() {
        let w = u32::from_le_bytes(*px) as u64 & 0x00FF_FFFF;
        lanes[i % 4] = (lanes[i % 4] ^ w).wrapping_mul(K).rotate_left(31);
    }
    let dims = (b.width() as u64) << 32 | b.height() as u64;
    lanes.iter().fold(dims.wrapping_mul(K), |acc, &l| (acc ^ l).wrapping_mul(K).rotate_left(27))
}

/// A bitmap's hash, size and exact-copy checksum.
#[derive(Clone, Debug)]
pub(crate) struct Fingerprint {
    pub hash: PerceptualHash,
    pub width: u32,
    pub height: u32,
    pub checksum: u64,
}

impl Fingerprint {
    pub fn of(b: &Bitmap) -> Self {
        Self { hash: perceptual_hash(b), width: b.width(), height: b.height(), checksum: content_checksum(b) }
    }
}

/// Compact features of a grid that change by at most as much as any cell
/// does: a row's max, its min, and its largest difference from a fixed pivot
/// row are all 1-Lipschitz under the max-cell distance. So two images whose
/// summaries differ by more than the tolerance anywhere cannot match, and
/// 96 contiguous bytes reject candidates that would otherwise cost a full
/// 2048-cell comparison. The pivot is any fixed grid shared by all summaries
/// being compared; one similar to the library makes the third feature sharp.
#[derive(Clone, Debug)]
pub(crate) struct Summary {
    pub width: u32,
    pub height: u32,
    /// Exact-copy checksum, kept here so rejecting an entry reads only this
    /// contiguous struct.
    pub checksum: u64,
    bounds: [u8; SUMMARY],
    spread: u8,
}

impl Summary {
    pub const FEATURES: usize = SUMMARY;

    pub fn new(fp: &Fingerprint, pivot: &PerceptualHash) -> Self {
        let Fingerprint { hash, width, height, checksum } = fp;
        let mut bounds = [0u8; SUMMARY];
        for (r, (row, prow)) in hash.rows().zip(pivot.rows()).enumerate() {
            bounds[r * 3] = row.iter().copied().fold(0, u8::max);
            bounds[r * 3 + 1] = row.iter().copied().fold(255, u8::min);
            bounds[r * 3 + 2] = max_diff(row, prow);
        }
        let brightest = bounds.iter().step_by(3).copied().fold(0, u8::max);
        let darkest = bounds.iter().skip(1).step_by(3).copied().fold(255, u8::min);
        Self { width: *width, height: *height, checksum: *checksum, bounds, spread: brightest.saturating_sub(darkest) }
    }

    /// Too little structure to tell screenshots apart; only exact copies
    /// match.
    fn is_flat(&self) -> bool {
        self.spread <= FLAT_SPREAD
    }

    /// The tolerance when compared with anything at least this large, or
    /// `None` when this image only matches exact copies.
    pub fn tolerance(&self) -> Option<u8> {
        tolerance(self, self)
    }

    pub fn feature(&self, i: usize) -> u8 {
        self.bounds[i]
    }

    /// Whether every feature is within `tol` of `other`'s.
    pub fn within(&self, other: &Summary, tol: u8) -> bool {
        max_diff(&self.bounds, &other.bounds) <= tol
    }
}

/// One side of a comparison, borrowed so scanning the index never touches
/// refcounts and only dereferences the grid when the summary passes.
#[derive(Clone, Copy)]
pub(crate) struct Probe<'a> {
    pub hash: &'a PerceptualHash,
    pub summary: &'a Summary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Verdict {
    /// Summed absolute difference over all cells; 0 for identical grids.
    pub distance: u32,
    pub exact: bool,
}

/// Two images match when every cell differs by at most the size-aware
/// tolerance. A single cell beyond it means content changed somewhere, which
/// is what separates "same screenshot" from "same app, different text".
pub(crate) fn compare(a: Probe<'_>, b: Probe<'_>) -> Option<Verdict> {
    let (sa, sb) = (a.summary, b.summary);
    if sa.width == sb.width && sa.height == sb.height && sa.checksum == sb.checksum && a.hash == b.hash {
        return Some(Verdict { distance: 0, exact: true });
    }
    let tol = tolerance(sa, sb)?;
    if max_diff(&sa.bounds, &sb.bounds) > tol || sa.is_flat() || sb.is_flat() || !aspect_close(sa, sb) {
        return None;
    }
    if a.hash.rows().zip(b.hash.rows()).any(|(ra, rb)| max_diff(ra, rb) > tol) {
        return None;
    }
    let distance = a.hash.0.iter().zip(b.hash.0.iter()).map(|(x, y)| x.abs_diff(*y) as u32).sum();
    Some(Verdict { distance, exact: false })
}

/// A fold rather than `any`, so it compiles to wide byte operations.
fn max_diff(a: &[u8], b: &[u8]) -> u8 {
    a.iter().zip(b).fold(0, |m, (x, y)| m.max(x.abs_diff(*y)))
}

fn aspect_close(a: &Summary, b: &Summary) -> bool {
    let ra = a.width as f32 / a.height as f32;
    let rb = b.width as f32 / b.height as f32;
    (ra - rb).abs() <= MAX_ASPECT_DELTA * ra.max(rb)
}

/// Largest per-cell difference still counted as the same image, from the
/// smaller image's cell size: the fewer pixels a cell covers, the more a
/// one-pixel shift of an edge changes its mean. Equals the larger of the two
/// images' own tolerances.
fn tolerance(a: &Summary, b: &Summary) -> Option<u8> {
    let cell_w = a.width.min(b.width) as f32 / GW as f32;
    let cell_h = a.height.min(b.height) as f32 / GH as f32;
    let cell_px = cell_w.min(cell_h);
    (cell_px >= MIN_CELL_PX).then(|| (BASE_TOLERANCE + EDGE_NOISE / cell_px) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::*;
    use sukurin_core::RectPx;

    /// A reference image, hashed once, that candidates are compared with.
    struct Reference {
        fp: Fingerprint,
        summary: Summary,
    }

    impl Reference {
        fn new(b: &Bitmap) -> Self {
            let fp = Fingerprint::of(b);
            let summary = Summary::new(&fp, &fp.hash);
            Self { fp, summary }
        }

        fn matches(&self, b: &Bitmap) -> Option<Verdict> {
            let other = Fingerprint::of(b);
            let summary = Summary::new(&other, &self.fp.hash);
            compare(
                Probe { hash: &self.fp.hash, summary: &self.summary },
                Probe { hash: &other.hash, summary: &summary },
            )
        }
    }

    fn matches(a: &Bitmap, b: &Bitmap) -> Option<Verdict> {
        Reference::new(a).matches(b)
    }

    const SIZES: [(u32, u32); 4] = [(2560, 1600), (1920, 1080), (800, 500), (480, 300)];

    #[test]
    fn same_screenshot_after_reencode_rescale_or_crop_matches() {
        for (w, h) in SIZES {
            let base = screen(w, h, &rows(24));
            let reference = Reference::new(&base);
            let half = resize(&base, w / 2, h / 2);
            let variants = [
                ("jpeg q80", reencode_jpeg(&base, 80)),
                ("50% downscale", half.clone()),
                ("50% down then up", resize(&half, w, h)),
                ("1px crop right/bottom", base.crop(RectPx::new(0, 0, w - 1, h - 1)).unwrap()),
                ("1px crop left/top", base.crop(RectPx::new(1, 1, w - 1, h - 1)).unwrap()),
                (
                    "crop, downscale and jpeg",
                    reencode_jpeg(&resize(&base.crop(RectPx::new(1, 1, w - 2, h - 2)).unwrap(), w / 2, h / 2), 80),
                ),
            ];
            for (name, v) in variants {
                let verdict = reference.matches(&v);
                assert!(matches!(verdict, Some(Verdict { exact: false, .. })), "{w}x{h} {name}: {verdict:?}");
            }
        }
    }

    #[test]
    fn same_layout_with_different_table_content_does_not_match() {
        for (w, h) in SIZES {
            let base_rows = rows(24);
            let reference = Reference::new(&screen(w, h, &base_rows));
            for i in 0..base_rows.len() {
                let mut r = base_rows.clone();
                r[i] = 5000 + i as u64;
                assert_eq!(reference.matches(&screen(w, h, &r)), None, "{w}x{h}: row {i} text changed");
            }
            let mut added = base_rows.clone();
            added.insert(10, 777);
            assert_eq!(reference.matches(&screen(w, h, &added)), None, "{w}x{h}: row added");
            let mut moved = base_rows.clone();
            moved.swap(3, 15);
            assert_eq!(reference.matches(&screen(w, h, &moved)), None, "{w}x{h}: rows swapped");
        }
    }

    #[test]
    fn blank_images_only_match_exact_copies() {
        let white = uniform(1920, 1080, [255, 255, 255]);
        let near_white = uniform(1920, 1080, [252, 252, 252]);
        assert_eq!(matches(&white, &near_white), None);
        assert_eq!(matches(&white, &uniform(1280, 720, [255, 255, 255])), None);
        assert_eq!(matches(&white, &screen(1920, 1080, &[])), None);
        assert_eq!(matches(&white, &white.clone()), Some(Verdict { distance: 0, exact: true }));
    }

    #[test]
    fn tiny_images_only_match_exact_copies() {
        let snip = resize(&screen(1200, 400, &rows(6)), 36, 12);
        let larger = screen(1200, 400, &rows(6));
        assert_eq!(matches(&snip, &larger), None);
        assert_eq!(matches(&larger, &snip), None);
        assert_eq!(matches(&snip, &resize(&snip, 35, 12)), None);
        let copy = Bitmap::from_bgra(36, 12, snip.bgra().to_vec());
        assert_eq!(matches(&snip, &copy), Some(Verdict { distance: 0, exact: true }));
    }

    #[test]
    fn different_aspect_ratio_does_not_match() {
        let base = screen(1920, 1080, &rows(24));
        let squashed = resize(&base, 1920, 1000);
        assert_eq!(matches(&base, &squashed), None);
    }

    #[test]
    fn exact_requires_identical_pixels() {
        let base = screen(800, 500, &rows(10));
        let copy = Bitmap::from_bgra(800, 500, base.bgra().to_vec());
        assert_eq!(matches(&base, &copy), Some(Verdict { distance: 0, exact: true }));
        let mut one_pixel = base.bgra().to_vec();
        one_pixel[4 * 1234] ^= 0x40;
        let verdict = matches(&base, &Bitmap::from_bgra(800, 500, one_pixel)).unwrap();
        assert!(!verdict.exact);
    }

    #[test]
    fn alpha_does_not_affect_checksum() {
        let base = screen(300, 200, &rows(4));
        let mut transparent = base.bgra().to_vec();
        transparent.iter_mut().skip(3).step_by(4).for_each(|a| *a = 0);
        assert_eq!(content_checksum(&base), content_checksum(&Bitmap::from_bgra(300, 200, transparent)));
    }

    #[test]
    fn tiny_images_hash_without_panicking() {
        for (w, h) in [(1, 1), (10, 3), (63, 31), (64, 32), (65, 33)] {
            let b = uniform(w, h, [10, 20, 30]);
            assert_eq!(perceptual_hash(&b).as_bytes().len(), CELLS);
        }
    }

    #[test]
    fn hash_round_trips_through_bytes() {
        let h = perceptual_hash(&screen(640, 400, &rows(8)));
        assert_eq!(PerceptualHash::from_bytes(h.as_bytes()), Some(h));
    }
}
