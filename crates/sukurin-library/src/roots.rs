use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    pub path: PathBuf,
    pub filter: RootFilter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootFilter {
    /// A folder that only holds screenshots: every image counts, two folder
    /// levels deep.
    AllImages,
    /// A general folder such as Desktop: only files named like screenshots,
    /// no subfolders.
    ScreenshotNamesOnly,
}

impl RootFilter {
    fn max_depth(self) -> usize {
        match self {
            RootFilter::AllImages => 2,
            RootFilter::ScreenshotNamesOnly => 0,
        }
    }
}

impl Root {
    pub fn all_images(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), filter: RootFilter::AllImages }
    }

    pub fn screenshot_names_only(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), filter: RootFilter::ScreenshotNamesOnly }
    }

    /// Subfolder depth of `path` below this root, if it is inside it.
    fn depth_of(&self, path: &Path) -> Option<usize> {
        let rel = path.strip_prefix(&self.path).ok()?;
        Some(rel.components().count().saturating_sub(1))
    }

    fn admits_name(&self, path: &Path) -> bool {
        has_image_extension(path)
            && match self.filter {
                RootFilter::AllImages => true,
                RootFilter::ScreenshotNamesOnly => path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(is_screenshot_name),
            }
    }
}

const MAX_FILE_BYTES: u64 = 60 * 1024 * 1024;

/// Change detector for a file: re-index only when either part moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Stamp {
    pub modified: SystemTime,
    pub bytes: u64,
}

impl Stamp {
    pub fn of(meta: &Metadata) -> Self {
        Self { modified: meta.modified().unwrap_or(UNIX_EPOCH), bytes: meta.len() }
    }
}

/// The configured roots, answering "does this file belong in the library".
#[derive(Clone, Debug)]
pub(crate) struct Roots(Vec<Root>);

impl Roots {
    pub fn new(roots: Vec<Root>) -> Self {
        Self(roots)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Root> {
        self.0.iter()
    }

    /// Whether a file at `path` with `meta` should be indexed.
    pub fn admits(&self, path: &Path, meta: &Metadata) -> bool {
        indexable(path, meta)
            && self.0.iter().any(|r| r.depth_of(path).is_some_and(|d| d <= r.filter.max_depth()) && r.admits_name(path))
    }

    /// Whether anything at `path` could matter: a file or folder inside a
    /// root's depth range. Cheap prefilter for watcher events.
    pub fn covers(&self, path: &Path) -> bool {
        self.0.iter().any(|r| r.depth_of(path).is_some_and(|d| d <= r.filter.max_depth()))
    }

    /// Every admitted file under all roots, or under `within` only when given.
    pub fn walk(&self, within: Option<&Path>) -> Vec<(PathBuf, Stamp)> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for root in &self.0 {
            let start = match within {
                None => root.path.clone(),
                Some(dir) if root.depth_of(dir).is_some() => dir.to_path_buf(),
                Some(dir) if root.path.starts_with(dir) => root.path.clone(),
                Some(_) => continue,
            };
            self.walk_dir(root, &start, &mut seen, &mut out);
        }
        out
    }

    fn walk_dir(&self, root: &Root, dir: &Path, seen: &mut HashSet<PathBuf>, out: &mut Vec<(PathBuf, Stamp)>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                let hidden = entry.file_name().to_str().is_none_or(|n| n.starts_with('.'));
                if !hidden && root.depth_of(&path).is_some_and(|d| d < root.filter.max_depth()) {
                    self.walk_dir(root, &path, seen, out);
                }
                continue;
            }
            if !root.admits_name(&path) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if self.admits(&path, &meta) && seen.insert(path.clone()) {
                out.push((path, Stamp::of(&meta)));
            }
        }
    }
}

/// Whether a file can be indexed at all, wherever it lives.
pub(crate) fn indexable(path: &Path, meta: &Metadata) -> bool {
    meta.is_file()
        && (1..=MAX_FILE_BYTES).contains(&meta.len())
        && !is_cloud_placeholder(meta)
        && path.to_str().is_some()
}

fn has_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["png", "jpg", "jpeg", "webp"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Default names of screenshot tools, including the localized macOS names.
const SCREENSHOT_PREFIXES: &[&str] = &[
    "screenshot",
    "screen shot",
    "capture",
    "cleanshot",
    "shottr",
    "snip",
    "sukurin-",
    "captura",
    "bildschirmfoto",
    "schermafbeelding",
    "skärmavbild",
    "zrzut ekranu",
    "スクリーンショット",
    "屏幕截图",
    "스크린샷",
];

pub(crate) fn is_screenshot_name(file_name: &str) -> bool {
    let lower = file_name.to_lowercase();
    SCREENSHOT_PREFIXES.iter().any(|p| lower.starts_with(p)) || starts_with_capture_timestamp(&lower)
}

/// ShareX/Greenshot `2026-10-05 14_12_00-…` and Flameshot `2026-10-05_14-12`.
/// Times separated by dots are left out: that is how cameras name photos.
fn starts_with_capture_timestamp(name: &str) -> bool {
    let b = name.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
    digits(0..4)
        && b.get(4) == Some(&b'-')
        && digits(5..7)
        && b.get(7) == Some(&b'-')
        && digits(8..10)
        && matches!(b.get(10), Some(b' ' | b'_'))
        && digits(11..13)
        && matches!(b.get(13), Some(b'_' | b'-'))
        && digits(14..16)
}

/// OneDrive "Files On-Demand" placeholders download when read; never touch
/// them for indexing.
#[cfg(windows)]
fn is_cloud_placeholder(meta: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x4_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    meta.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

#[cfg(not(windows))]
fn is_cloud_placeholder(_meta: &Metadata) -> bool {
    false
}

/// Screenshot folders that exist on this machine, deduplicated.
pub fn default_roots() -> Vec<Root> {
    let mut roots = platform_roots();
    if let Some(pictures) = dirs::picture_dir() {
        roots.push(Root::all_images(pictures.join("Sukurin")));
    }
    let mut seen = HashSet::new();
    roots.sort_by_key(|r| r.filter != RootFilter::AllImages);
    roots.retain(|r| r.path.is_dir() && seen.insert(fs::canonicalize(&r.path).unwrap_or_else(|_| r.path.clone())));
    roots
}

#[cfg(windows)]
fn platform_roots() -> Vec<Root> {
    let mut roots = Vec::new();
    let pictures = dirs::picture_dir();
    if let Some(p) = &pictures {
        roots.push(Root::all_images(p.join("Screenshots")));
    }
    let mut onedrives: Vec<PathBuf> = ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    if let Some(home) = dirs::home_dir()
        && let Ok(read) = fs::read_dir(&home)
    {
        onedrives.extend(
            read.flatten()
                .filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with("OneDrive")))
                .map(|e| e.path()),
        );
    }
    roots.extend(onedrives.iter().map(|od| Root::all_images(od.join("Pictures").join("Screenshots"))));
    if let Some(videos) = dirs::video_dir() {
        roots.push(Root::all_images(videos.join("Captures")));
    }
    if let Some(docs) = dirs::document_dir() {
        roots.push(Root::all_images(docs.join("ShareX").join("Screenshots")));
    }
    if let Some(desktop) = dirs::desktop_dir() {
        roots.push(Root::screenshot_names_only(desktop));
    }
    if let Some(p) = pictures {
        roots.push(Root::screenshot_names_only(p));
    }
    roots
}

#[cfg(target_os = "macos")]
fn platform_roots() -> Vec<Root> {
    let mut roots = Vec::new();
    if let Some(location) = screencapture_location() {
        let general = [dirs::home_dir(), dirs::desktop_dir(), dirs::document_dir(), dirs::download_dir(), dirs::picture_dir()];
        let is_general = general.iter().flatten().any(|g| *g == location);
        roots.push(if is_general { Root::screenshot_names_only(location) } else { Root::all_images(location) });
    }
    if let Some(p) = dirs::picture_dir() {
        roots.push(Root::all_images(p.join("Screenshots")));
    }
    if let Some(d) = dirs::desktop_dir() {
        roots.push(Root::screenshot_names_only(d));
    }
    roots
}

#[cfg(target_os = "macos")]
fn screencapture_location() -> Option<PathBuf> {
    let out = std::process::Command::new("defaults")
        .args(["read", "com.apple.screencapture", "location"])
        .output()
        .ok()?;
    let raw = String::from_utf8(out.stdout).ok()?;
    let raw = raw.trim();
    if !out.status.success() || raw.is_empty() {
        return None;
    }
    Some(match raw.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()?.join(rest),
        None => PathBuf::from(raw),
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_roots() -> Vec<Root> {
    let mut roots = Vec::new();
    if let Some(p) = dirs::picture_dir() {
        roots.push(Root::all_images(p.join("Screenshots")));
        roots.push(Root::screenshot_names_only(p));
    }
    roots
}

#[cfg(not(any(windows, unix)))]
fn platform_roots() -> Vec<Root> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_names() {
        for yes in [
            "Screenshot 2026-10-05 141200.png",
            "Screen Shot 2020-01-02 at 10.11.12.png",
            "Screenshot_20231005_141200.png",
            "screenshot from 2023-10-05 14-12-00.png",
            "Capture.PNG",
            "CleanShot 2024-01-01 at 10.00.00@2x.png",
            "Shottr 2024.png",
            "Snipaste_2023-10-05_14-12-00.png",
            "sukurin-20261005-141200.png",
            "2026-10-05 14_12_00-Visual Studio Code.png",
            "2026-10-05_14-12.png",
            "Bildschirmfoto 2023-10-05 um 14.12.00.png",
        ] {
            assert!(is_screenshot_name(yes), "{yes}");
        }
        for no in ["holiday.jpg", "IMG_1234.jpg", "2026-10-05 14.12.33.jpg", "my screenshot.png", "2026-10-05.png"] {
            assert!(!is_screenshot_name(no), "{no}");
        }
    }

    #[test]
    fn admits_respects_filter_extension_and_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let desk = tmp.path().join("Desktop");
        let shots = tmp.path().join("Shots");
        let roots = Roots::new(vec![Root::screenshot_names_only(&desk), Root::all_images(&shots)]);
        let write = |p: &Path| {
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"x").unwrap();
            fs::metadata(p).unwrap()
        };
        let cases = [
            (desk.join("Screenshot 1.png"), true),
            (desk.join("holiday.jpg"), false),
            (desk.join("sub").join("Screenshot 2.png"), false),
            (desk.join("Screenshot 3.txt"), false),
            (shots.join("holiday.jpg"), true),
            (shots.join("a").join("b").join("deep.webp"), true),
            (shots.join("a").join("b").join("c").join("too-deep.png"), false),
        ];
        for (path, expected) in cases {
            let meta = write(&path);
            assert_eq!(roots.admits(&path, &meta), expected, "{}", path.display());
        }
        let mut walked: Vec<PathBuf> = roots.walk(None).into_iter().map(|(p, _)| p).collect();
        walked.sort();
        let mut expected = vec![desk.join("Screenshot 1.png"), shots.join("a").join("b").join("deep.webp"), shots.join("holiday.jpg")];
        expected.sort();
        assert_eq!(walked, expected);
        let within: Vec<PathBuf> = roots.walk(Some(&shots.join("a"))).into_iter().map(|(p, _)| p).collect();
        assert_eq!(within, vec![shots.join("a").join("b").join("deep.webp")]);
    }

    #[test]
    fn empty_files_are_not_admitted() {
        let tmp = tempfile::tempdir().unwrap();
        let roots = Roots::new(vec![Root::all_images(tmp.path())]);
        let p = tmp.path().join("empty.png");
        fs::write(&p, b"").unwrap();
        assert!(!roots.admits(&p, &fs::metadata(&p).unwrap()));
    }
}
