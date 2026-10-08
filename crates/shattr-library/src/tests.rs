use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::Receiver;
use shattr_core::Bitmap;
use tempfile::TempDir;

use crate::synth::{self, rows, screen};
use crate::*;

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self { dir: tempfile::tempdir().unwrap() }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn open(&self, roots: Vec<Root>) -> Library {
        Library::open(&self.path("db/library.sqlite"), &self.path("thumbs"), roots).unwrap()
    }

    fn write(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let p = self.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
        p
    }
}

fn shot(seed: u64) -> Bitmap {
    let mut r = rows(16);
    r[3] = seed;
    r[9] = seed + 1;
    screen(640, 400, &r)
}

fn scan(lib: &Library) -> Vec<LibraryEvent> {
    let rx = lib.subscribe();
    lib.start();
    wait_for(&rx, |e| matches!(e, LibraryEvent::ScanFinished))
}

fn wait_for(rx: &Receiver<LibraryEvent>, done: impl Fn(&LibraryEvent) -> bool) -> Vec<LibraryEvent> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen = Vec::new();
    while let Ok(e) = rx.recv_deadline(deadline) {
        let finished = done(&e);
        seen.push(e);
        if finished {
            return seen;
        }
    }
    panic!("timed out; events so far: {seen:?}");
}

fn paths(lib: &Library) -> Vec<PathBuf> {
    let mut p: Vec<PathBuf> = lib.entries().into_iter().map(|e| e.path).collect();
    p.sort();
    p
}

#[test]
fn names_only_root_indexes_screenshot_names_and_ignores_other_images() {
    let fx = Fixture::new();
    let shot_path = fx.write("Desktop/Screenshot 2026-10-05 141200.png", &synth::png_bytes(&shot(1)));
    fx.write("Desktop/holiday.jpg", &synth::jpeg_bytes(&shot(2), 80));
    fx.write("Desktop/nested/Screenshot 2026-10-06 090000.png", &synth::png_bytes(&shot(3)));
    let lib = fx.open(vec![Root::screenshot_names_only(fx.path("Desktop"))]);

    scan(&lib);

    assert_eq!(paths(&lib), vec![shot_path]);
    assert_eq!(lib.status(), IndexStatus { indexed: 1, pending: 0, scanning: false });
}

#[test]
fn all_images_root_indexes_every_image_two_levels_deep() {
    let fx = Fixture::new();
    let expected = vec![
        fx.write("Shots/a/b/deep.jpeg", &synth::jpeg_bytes(&shot(1), 80)),
        fx.write("Shots/holiday.jpg", &synth::jpeg_bytes(&shot(2), 80)),
        fx.write("Shots/sub/whatever.png", &synth::png_bytes(&shot(3))),
    ];
    fx.write("Shots/a/b/c/too-deep.png", &synth::png_bytes(&shot(4)));
    fx.write("Shots/notes.txt", b"not an image");
    fx.write("Shots/broken.png", b"not a png either");
    let lib = fx.open(vec![Root::all_images(fx.path("Shots"))]);

    scan(&lib);

    assert_eq!(paths(&lib), expected);
    let entry = lib.entries().into_iter().find(|e| e.path.ends_with("whatever.png")).unwrap();
    assert_eq!((entry.width, entry.height), (640, 400));
    let thumb = image::open(entry.thumb.expect("thumbnail written")).unwrap();
    assert_eq!((thumb.width(), thumb.height()), (320, 200));
}

#[test]
fn reopening_skips_unchanged_files_and_reindexes_changed_ones() {
    let fx = Fixture::new();
    let roots = || vec![Root::all_images(fx.path("Shots"))];
    fx.write("Shots/one.png", &synth::png_bytes(&shot(1)));
    let changed = fx.write("Shots/two.png", &synth::png_bytes(&shot(2)));
    fx.write("Shots/three.jpg", &synth::jpeg_bytes(&shot(3), 80));
    let first = fx.open(roots());
    scan(&first);
    assert_eq!(first.decode_count(), 3);
    let ids: Vec<i64> = first.entries().iter().map(|e| e.id).collect();
    drop(first);

    let reopened = fx.open(roots());
    assert_eq!(reopened.status().indexed, 3, "index loads from the database before any scan");
    scan(&reopened);
    assert_eq!(reopened.decode_count(), 0);
    assert_eq!(reopened.entries().iter().map(|e| e.id).collect::<Vec<_>>(), ids);
    drop(reopened);

    fs::write(&changed, synth::png_bytes(&screen(640, 400, &rows(3)))).unwrap();
    let after_edit = fx.open(roots());
    let events = scan(&after_edit);
    assert_eq!(after_edit.decode_count(), 1);
    let reindexed: Vec<&PathBuf> = events
        .iter()
        .filter_map(|e| match e {
            LibraryEvent::Indexed(entry) => Some(&entry.path),
            _ => None,
        })
        .collect();
    assert_eq!(reindexed, vec![&changed]);
}

#[test]
fn rescan_removes_rows_and_thumbnails_of_deleted_files() {
    let fx = Fixture::new();
    let roots = || vec![Root::all_images(fx.path("Shots"))];
    let keep = fx.write("Shots/keep.png", &synth::png_bytes(&shot(1)));
    let gone = fx.write("Shots/gone.png", &synth::png_bytes(&shot(2)));
    let first = fx.open(roots());
    scan(&first);
    let gone_entry = first.entries().into_iter().find(|e| e.path == gone).unwrap();
    let gone_thumb = gone_entry.thumb.clone().unwrap();
    assert!(gone_thumb.exists());
    drop(first);
    fs::remove_file(&gone).unwrap();

    let lib = fx.open(roots());
    let events = scan(&lib);

    assert_eq!(paths(&lib), vec![keep]);
    assert!(!gone_thumb.exists());
    assert!(events.iter().any(|e| matches!(e, LibraryEvent::Removed { id, path } if *id == gone_entry.id && *path == gone)));
    drop(lib);
    assert_eq!(fx.open(roots()).status().indexed, 1, "removal is persisted");
}

#[test]
fn watcher_indexes_new_files_and_drops_deleted_ones() {
    let fx = Fixture::new();
    fs::create_dir_all(fx.path("Shots")).unwrap();
    let lib = fx.open(vec![Root::all_images(fx.path("Shots"))]);
    scan(&lib);
    let rx = lib.subscribe();

    let created = fx.write("Shots/new.png", &synth::png_bytes(&shot(5)));
    wait_for(&rx, |e| matches!(e, LibraryEvent::Indexed(entry) if entry.path == created));
    assert_eq!(paths(&lib), vec![created.clone()]);

    fs::remove_file(&created).unwrap();
    wait_for(&rx, |e| matches!(e, LibraryEvent::Removed { path, .. } if *path == created));
    assert!(lib.entries().is_empty());
}

#[test]
fn recorded_capture_is_found_as_exact_without_decoding() {
    let fx = Fixture::new();
    let lib = fx.open(vec![]);
    let capture = shot(7);
    let path = fx.write("Pictures/Shattr/shattr-20261007-101500.png", &synth::png_bytes(&capture));

    let recorded = lib.record(&path, &capture).unwrap();
    let hits = lib.similar(&capture, 5);

    assert_eq!(lib.decode_count(), 0);
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].entry.id, hits[0].distance, hits[0].exact), (recorded.id, 0, true));
    assert_eq!(hits[0].entry.path, path);
    assert!(recorded.thumb.is_some_and(|t| t.exists()));
    assert!(lib.similar(&shot(8), 5).is_empty(), "a different capture of the same layout is not similar");
}

#[test]
fn record_requires_the_file_to_exist() {
    let fx = Fixture::new();
    let lib = fx.open(vec![]);
    assert!(lib.record(&fx.path("missing.png"), &shot(1)).is_err());
    assert_eq!(lib.status().indexed, 0);
}

#[test]
fn jpeg_copy_groups_with_its_png_original() {
    let fx = Fixture::new();
    let original = shot(1);
    let png = fx.write("Shots/Screenshot 1.png", &synth::png_bytes(&original));
    let jpg = fx.write("Shots/copy of screenshot.jpg", &synth::jpeg_bytes(&original, 80));
    fx.write("Shots/Screenshot 2.png", &synth::png_bytes(&shot(2)));
    fx.write("Shots/Screenshot 3.png", &synth::png_bytes(&synth::uniform(640, 400, [255, 255, 255])));
    let lib = fx.open(vec![Root::all_images(fx.path("Shots"))]);
    scan(&lib);

    let groups = lib.duplicate_groups();
    assert_eq!(groups.len(), 1);
    let mut grouped: Vec<PathBuf> = groups[0].iter().map(|e| e.path.clone()).collect();
    grouped.sort();
    assert_eq!(grouped, vec![png.clone(), jpg.clone()]);

    let hits = lib.similar(&original, 10);
    let found: Vec<(&Path, bool)> = hits.iter().map(|h| (h.entry.path.as_path(), h.exact)).collect();
    assert_eq!(found, vec![(png.as_path(), true), (jpg.as_path(), false)]);
}

#[test]
fn entries_are_newest_first() {
    let fx = Fixture::new();
    let lib = fx.open(vec![]);
    let older = fx.write("a.png", &synth::png_bytes(&shot(1)));
    let newer = fx.write("b.png", &synth::png_bytes(&shot(2)));
    let past = SystemTime::now() - Duration::from_secs(3600);
    fs::File::options().write(true).open(&older).unwrap().set_modified(past).unwrap();
    lib.record(&newer, &shot(2)).unwrap();
    lib.record(&older, &shot(1)).unwrap();
    assert_eq!(lib.entries().into_iter().map(|e| e.path).collect::<Vec<_>>(), vec![newer, older]);
}

#[test]
fn library_handle_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync + Clone>() {}
    assert_send_sync::<Library>();
}

fn median(mut v: Vec<Duration>) -> (Duration, Duration, Duration) {
    v.sort();
    (v[0], v[v.len() / 2], v[v.len() - 1])
}

/// `cargo test --release -p shattr-library -- --ignored --nocapture bench`
#[test]
#[ignore]
fn bench() {
    use crate::hash::{Fingerprint, perceptual_hash};
    use crate::index::{Index, Item};

    let big = screen(2560, 1600, &rows(40));
    let hash_runs: Vec<Duration> = (0..50)
        .map(|_| {
            let t = Instant::now();
            std::hint::black_box(perceptual_hash(std::hint::black_box(&big)));
            t.elapsed()
        })
        .collect();
    let (lo, mid, hi) = median(hash_runs);
    println!("perceptual_hash 2560x1600: median {mid:?} (min {lo:?}, max {hi:?}, 50 runs)");
    let fp_runs: Vec<Duration> = (0..50)
        .map(|_| {
            let t = Instant::now();
            std::hint::black_box(Fingerprint::of(std::hint::black_box(&big)));
            t.elapsed()
        })
        .collect();
    let (lo, mid, hi) = median(fp_runs);
    println!("hash + checksum 2560x1600: median {mid:?} (min {lo:?}, max {hi:?}, 50 runs)");

    let base_rows = rows(24);
    let t = Instant::now();
    let items: Vec<Item> = (0..20_000u64)
        .map(|i| {
            let mut r = base_rows.clone();
            let mut rng = synth::Rng::new(i);
            for _ in 0..1 + rng.below(3) {
                let row = rng.below(r.len() as u64) as usize;
                r[row] = rng.next();
            }
            let b = screen(640, 400, &r);
            let fp = Fingerprint::of(&b);
            let entry = Entry {
                id: i as i64,
                path: PathBuf::from(format!("/shots/{i}.png")),
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(i),
                bytes: 1,
                width: 640,
                height: 400,
                hash: fp.hash,
                thumb: None,
            };
            Item { entry, checksum: fp.checksum }
        })
        .collect();
    println!("built 20k same-layout entries in {:?}", t.elapsed());
    let index = Index::new(items);
    let probe = Fingerprint::of(&screen(640, 400, &base_rows));
    let mut hits = 0;
    let scan_runs: Vec<Duration> = (0..30)
        .map(|_| {
            let t = Instant::now();
            hits = std::hint::black_box(index.similar(&probe, 10)).len();
            t.elapsed()
        })
        .collect();
    let (lo, mid, hi) = median(scan_runs);
    println!("similar() scan over 20k same-layout entries: median {mid:?} (min {lo:?}, max {hi:?}, 30 runs), {hits} hits");
    let unrelated = Fingerprint::of(&synth::resize(&big, 640, 400));
    let scan_runs: Vec<Duration> = (0..30)
        .map(|_| {
            let t = Instant::now();
            std::hint::black_box(index.similar(&unrelated, 10));
            t.elapsed()
        })
        .collect();
    let (lo, mid, hi) = median(scan_runs);
    println!("similar() scan, probe unlike the library: median {mid:?} (min {lo:?}, max {hi:?}, 30 runs)");
    let t = Instant::now();
    let groups = index.duplicate_groups();
    println!(
        "duplicate_groups over 20k same-aspect entries: {:?}, {} groups, {} entries grouped",
        t.elapsed(),
        groups.len(),
        groups.iter().map(Vec::len).sum::<usize>()
    );
}

/// Read-only scan of this machine's real screenshot folders. The database
/// and thumbnails go to a temporary directory.
/// `cargo test --release -p shattr-library -- --ignored --nocapture real_machine_scan`
#[test]
#[ignore]
fn real_machine_scan() {
    let roots = default_roots();
    for r in &roots {
        println!("root {:?} {}", r.filter, r.path.display());
    }
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(&tmp.path().join("library.sqlite"), &tmp.path().join("thumbs"), roots).unwrap();
    let rx = lib.subscribe();
    let t = Instant::now();
    lib.start();
    let deadline = Instant::now() + Duration::from_secs(1800);
    while !matches!(rx.recv_deadline(deadline), Ok(LibraryEvent::ScanFinished)) {}
    println!("scan took {:?}, decoded {} files", t.elapsed(), lib.decode_count());
    let entries = lib.entries();
    let groups = lib.duplicate_groups();
    println!("{} entries, {} duplicate groups covering {} files", entries.len(), groups.len(), groups.iter().map(Vec::len).sum::<usize>());
    for g in groups.iter().take(5) {
        println!("group: {:?}", g.iter().map(|e| e.path.file_name().unwrap().to_owned()).collect::<Vec<_>>());
    }
    drop(lib);
    let t = Instant::now();
    let reopened = Library::open(&tmp.path().join("library.sqlite"), &tmp.path().join("thumbs"), default_roots()).unwrap();
    println!("reopen with {} entries took {:?}", reopened.status().indexed, t.elapsed());
}
