//! The screenshot library: finds screenshots already on disk, indexes them
//! by a perceptual hash, and answers whether a new capture duplicates one.

mod hash;
mod imaging;
mod index;
mod roots;
mod store;
#[cfg(test)]
mod synth;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fs;
use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::SystemTime;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::{Mutex, RwLock};
use sukurin_core::Bitmap;

use hash::Fingerprint;
use index::{Index, Item};
use roots::{Roots, Stamp};
use store::{Row, Store};

pub use hash::{PerceptualHash, perceptual_hash};
pub use roots::{Root, RootFilter, default_roots};

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: i64,
    pub path: PathBuf,
    pub modified: SystemTime,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub hash: PerceptualHash,
    pub thumb: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Similar {
    pub entry: Entry,
    /// Summed per-cell luminance difference of the 64x32 grids; 0 when the
    /// grids are identical. Only for ranking: whether two images match is
    /// decided per cell, not by a threshold on this sum.
    pub distance: u32,
    /// Same dimensions and identical colour pixels.
    pub exact: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IndexStatus {
    pub indexed: usize,
    /// Files queued for (re)indexing or removal.
    pub pending: usize,
    /// True from `start` until the first full scan has been indexed.
    pub scanning: bool,
}

#[derive(Clone, Debug)]
pub enum LibraryEvent {
    Indexed(Entry),
    Removed { id: i64, path: PathBuf },
    ScanFinished,
}

/// Cheap to clone; all clones share one index. Background threads hold only
/// weak references, so dropping the last handle stops them.
#[derive(Clone)]
pub struct Library {
    shared: Arc<Shared>,
}

const WORKERS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Walking,
    Draining,
    Watching,
}

/// Paths waiting to be reconciled with the index. Each path always goes to
/// the same worker, so two jobs for one file never race each other.
struct Queue {
    workers: [Sender<PathBuf>; WORKERS],
    pending: AtomicUsize,
    route: RandomState,
}

impl Queue {
    fn push(&self, path: PathBuf) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        let worker = self.route.hash_one(&path) as usize % WORKERS;
        let _ = self.workers[worker].send(path);
    }
}

struct Shared {
    store: Mutex<Store>,
    index: RwLock<Index>,
    roots: Roots,
    queue: Arc<Queue>,
    receivers: Mutex<Option<[Receiver<PathBuf>; WORKERS]>>,
    phase: Mutex<Phase>,
    subscribers: Mutex<Vec<Sender<LibraryEvent>>>,
    watcher: Mutex<Option<RecommendedWatcher>>,
    decodes: AtomicUsize,
}

impl Library {
    pub fn open(db_path: &Path, thumb_dir: &Path, roots: Vec<Root>) -> Result<Library> {
        let store = Store::open(db_path, thumb_dir)?;
        let index = Index::new(store.load()?);
        let (senders, receivers): (Vec<_>, Vec<_>) = (0..WORKERS).map(|_| crossbeam_channel::unbounded()).unzip();
        let shared = Shared {
            store: Mutex::new(store),
            index: RwLock::new(index),
            roots: Roots::new(roots),
            queue: Arc::new(Queue {
                workers: senders.try_into().unwrap(),
                pending: AtomicUsize::new(0),
                route: RandomState::new(),
            }),
            receivers: Mutex::new(Some(receivers.try_into().unwrap())),
            phase: Mutex::new(Phase::Idle),
            subscribers: Mutex::new(Vec::new()),
            watcher: Mutex::new(None),
            decodes: AtomicUsize::new(0),
        };
        Ok(Library { shared: Arc::new(shared) })
    }

    /// Starts the background scan and file watching. Returns immediately;
    /// calling it again does nothing.
    pub fn start(&self) {
        let Some([first, second]) = self.shared.receivers.lock().take() else { return };
        *self.shared.phase.lock() = Phase::Walking;
        let scan = Arc::downgrade(&self.shared);
        let index = Arc::downgrade(&self.shared);
        spawn("sukurin-library-scan", move || {
            if let Some(shared) = scan.upgrade() {
                shared.watch();
                shared.initial_scan();
            }
            work(scan, first);
        });
        spawn("sukurin-library-index", move || work(index, second));
    }

    /// Indexed screenshots that look like `b`, closest first, at most
    /// `limit`. Scans the in-memory index; never touches disk.
    pub fn similar(&self, b: &Bitmap, limit: usize) -> Vec<Similar> {
        let probe = Fingerprint::of(b);
        self.shared.index.read().similar(&probe, limit)
    }

    /// Every indexed screenshot, newest first.
    pub fn entries(&self) -> Vec<Entry> {
        self.shared.index.read().entries()
    }

    /// Sets of two or more screenshots that match each other, largest set
    /// first, each newest first.
    pub fn duplicate_groups(&self) -> Vec<Vec<Entry>> {
        self.shared.index.read().duplicate_groups()
    }

    /// Indexes a file the app just wrote, from the bitmap it wrote, without
    /// decoding the file.
    pub fn record(&self, path: &Path, b: &Bitmap) -> Result<Entry> {
        let path = std::path::absolute(path)?;
        let meta = fs::metadata(&path).with_context(|| format!("reading {}", path.display()))?;
        self.shared.index_bitmap(&path, Stamp::of(&meta), b)
    }

    pub fn status(&self) -> IndexStatus {
        IndexStatus {
            indexed: self.shared.index.read().len(),
            pending: self.shared.queue.pending.load(Ordering::SeqCst),
            scanning: matches!(*self.shared.phase.lock(), Phase::Walking | Phase::Draining),
        }
    }

    pub fn subscribe(&self) -> Receiver<LibraryEvent> {
        let (tx, rx) = crossbeam_channel::unbounded();
        self.shared.subscribers.lock().push(tx);
        rx
    }

    #[cfg(test)]
    fn decode_count(&self) -> usize {
        self.shared.decodes.load(Ordering::SeqCst)
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().name(name.into()).spawn(f).expect("spawning library thread");
}

/// Reconciles queued paths until every `Library` handle is gone. Bursts of
/// events for one file (a tool writing it in chunks) collapse to one sync.
fn work(shared: Weak<Shared>, jobs: Receiver<PathBuf>) {
    while let Ok(first) = jobs.recv() {
        let batch: Vec<PathBuf> = std::iter::once(first).chain(jobs.try_iter()).collect();
        let mut seen = HashSet::new();
        for path in batch {
            let Some(shared) = shared.upgrade() else { return };
            if seen.insert(path.clone()) {
                shared.sync(&path);
            }
            shared.queue.pending.fetch_sub(1, Ordering::SeqCst);
            shared.finish_scan_if_drained();
        }
    }
}

impl Shared {
    fn watch(&self) {
        let roots = self.roots.clone();
        let queue = Arc::clone(&self.queue);
        let handler = move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            for path in event.paths {
                if roots.covers(&path) {
                    queue.push(path);
                }
            }
        };
        let Ok(mut watcher) = notify::recommended_watcher(handler) else { return };
        for root in self.roots.iter() {
            let mode = match root.filter {
                RootFilter::AllImages => RecursiveMode::Recursive,
                RootFilter::ScreenshotNamesOnly => RecursiveMode::NonRecursive,
            };
            let _ = watcher.watch(&root.path, mode);
        }
        *self.watcher.lock() = Some(watcher);
    }

    /// Queues every new or changed file under the roots, and every indexed
    /// file the walk did not see (deleted, or recorded outside the roots).
    fn initial_scan(&self) {
        let mut known = self.index.read().stamps();
        for (path, stamp) in self.roots.walk(None) {
            if known.remove(&path) != Some(stamp) {
                self.queue.push(path);
            }
        }
        for path in known.into_keys() {
            self.queue.push(path);
        }
        *self.phase.lock() = Phase::Draining;
        self.finish_scan_if_drained();
    }

    fn finish_scan_if_drained(&self) {
        let mut phase = self.phase.lock();
        if *phase == Phase::Draining && self.queue.pending.load(Ordering::SeqCst) == 0 {
            *phase = Phase::Watching;
            drop(phase);
            self.broadcast(LibraryEvent::ScanFinished);
        }
    }

    /// Makes the index agree with whatever is at `path` now. Safe to repeat:
    /// an unchanged file is skipped by its stamp, a missing one is removed.
    fn sync(&self, path: &Path) {
        let Ok(meta) = fs::metadata(path) else {
            self.remove_under(path);
            return;
        };
        if meta.is_dir() {
            for (file, _) in self.roots.walk(Some(path)) {
                self.queue.push(file);
            }
            return;
        }
        let stamp = Stamp::of(&meta);
        let known = self.index.read().get(path).map(Item::stamp);
        if known == Some(stamp) {
            return;
        }
        let wanted = match known {
            Some(_) => roots::indexable(path, &meta),
            None => self.roots.admits(path, &meta),
        };
        if !wanted {
            return;
        }
        self.decodes.fetch_add(1, Ordering::SeqCst);
        if let Ok(bitmap) = imaging::decode(path) {
            let _ = self.index_bitmap(path, stamp, &bitmap);
        }
    }

    fn index_bitmap(&self, path: &Path, stamp: Stamp, b: &Bitmap) -> Result<Entry> {
        let path_str = path.to_str().with_context(|| format!("non-UTF-8 path {}", path.display()))?;
        let fp = Fingerprint::of(b);
        let thumb = imaging::thumbnail_jpeg(b).ok();
        let row = Row { path: path_str, stamp, width: b.width(), height: b.height(), hash: &fp.hash, checksum: fp.checksum };
        let entry = {
            let store = self.store.lock();
            let entry = store.upsert(row, thumb.as_deref())?;
            self.index.write().upsert(Item { entry: entry.clone(), checksum: fp.checksum });
            entry
        };
        self.broadcast(LibraryEvent::Indexed(entry.clone()));
        Ok(entry)
    }

    fn remove_under(&self, path: &Path) {
        let removed = {
            let store = self.store.lock();
            let removed = self.index.write().remove_under(path);
            for item in &removed {
                let _ = store.delete(item.entry.id);
            }
            removed
        };
        for item in removed {
            self.broadcast(LibraryEvent::Removed { id: item.entry.id, path: item.entry.path });
        }
    }

    fn broadcast(&self, event: LibraryEvent) {
        self.subscribers.lock().retain(|tx| tx.send(event.clone()).is_ok());
    }
}
