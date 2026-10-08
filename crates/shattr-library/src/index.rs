use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::hash::{Fingerprint, PerceptualHash, Probe, Summary, Verdict, compare};
use crate::roots::Stamp;
use crate::{Entry, Similar};

/// One indexed file: the public entry plus the exact-copy checksum, which
/// only the matcher needs.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub entry: Entry,
    pub checksum: u64,
}

impl Item {
    pub fn stamp(&self) -> Stamp {
        Stamp { modified: self.entry.modified, bytes: self.entry.bytes }
    }
}

/// Tolerance above which an image is too small for the windowed sweep in
/// `duplicate_groups` to stay narrow (under about 480x240 px).
const SWEEP_TOLERANCE_CAP: u8 = 24;

/// In-memory mirror of the store, laid out for scanning: summaries sit in
/// their own dense Vec so `similar` streams ~100 bytes per entry and only
/// touches a full grid for the few entries that pass.
#[derive(Default)]
pub(crate) struct Index {
    items: Vec<Item>,
    summaries: Vec<Summary>,
    by_path: HashMap<PathBuf, usize>,
    /// Reference grid for every summary in this index: the first image
    /// indexed. Kept even if that file goes away; it is only a fixed point.
    pivot: Option<PerceptualHash>,
}

impl Index {
    pub fn new(items: Vec<Item>) -> Self {
        let mut index = Self::default();
        for item in items {
            index.upsert(item);
        }
        index
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn get(&self, path: &Path) -> Option<&Item> {
        self.by_path.get(path).map(|&i| &self.items[i])
    }

    pub fn stamps(&self) -> HashMap<PathBuf, Stamp> {
        self.items.iter().map(|it| (it.entry.path.clone(), it.stamp())).collect()
    }

    pub fn upsert(&mut self, item: Item) {
        let pivot = self.pivot.get_or_insert_with(|| item.entry.hash.clone());
        let fp = Fingerprint {
            hash: item.entry.hash.clone(),
            width: item.entry.width,
            height: item.entry.height,
            checksum: item.checksum,
        };
        let summary = Summary::new(&fp, pivot);
        match self.by_path.get(&item.entry.path) {
            Some(&i) => {
                self.items[i] = item;
                self.summaries[i] = summary;
            }
            None => {
                self.by_path.insert(item.entry.path.clone(), self.items.len());
                self.items.push(item);
                self.summaries.push(summary);
            }
        }
    }

    /// Removes the item at `path` and every item below it, for a deleted
    /// file or a deleted folder alike.
    pub fn remove_under(&mut self, path: &Path) -> Vec<Item> {
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].entry.path.starts_with(path) {
                let item = self.items.swap_remove(i);
                self.summaries.swap_remove(i);
                self.by_path.remove(&item.entry.path);
                if let Some(moved) = self.items.get(i) {
                    self.by_path.insert(moved.entry.path.clone(), i);
                }
                removed.push(item);
            } else {
                i += 1;
            }
        }
        removed
    }

    pub fn entries(&self) -> Vec<Entry> {
        let mut out: Vec<Entry> = self.items.iter().map(|it| it.entry.clone()).collect();
        out.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.id.cmp(&a.id)));
        out
    }

    fn probe(&self, i: usize) -> Probe<'_> {
        Probe { hash: &self.items[i].entry.hash, summary: &self.summaries[i] }
    }

    pub fn similar(&self, query: &Fingerprint, limit: usize) -> Vec<Similar> {
        let Some(pivot) = &self.pivot else { return Vec::new() };
        let summary = Summary::new(query, pivot);
        let probe = Probe { hash: &query.hash, summary: &summary };
        let mut hits: Vec<(Verdict, &Item)> = (0..self.items.len())
            .filter_map(|i| compare(probe, self.probe(i)).map(|v| (v, &self.items[i])))
            .collect();
        hits.sort_by_key(|(v, it)| (v.distance, !v.exact, std::cmp::Reverse(it.entry.modified)));
        hits.truncate(limit);
        hits.into_iter()
            .map(|(v, it)| Similar { entry: it.entry.clone(), distance: v.distance, exact: v.exact })
            .collect()
    }

    /// Connected components of the "matches" relation, largest first.
    ///
    /// A match needs every summary feature within the pair's tolerance, so
    /// after sorting by the feature that varies most, each entry only has to
    /// be checked against the run of entries whose feature is within the
    /// largest tolerance, first on a sorted copy of the summaries so the
    /// common rejection reads contiguous memory. Small images have a
    /// tolerance that would widen that run for everyone; they are compared
    /// with all entries instead.
    pub fn duplicate_groups(&self) -> Vec<Vec<Entry>> {
        let n = self.items.len();
        let (small, regular): (Vec<usize>, Vec<usize>) =
            (0..n).partition(|&i| self.summaries[i].tolerance().is_none_or(|t| t > SWEEP_TOLERANCE_CAP));
        let window = regular.iter().filter_map(|&i| self.summaries[i].tolerance()).max().unwrap_or(0);
        let key = self.most_varied_feature(&regular);
        let mut order = regular;
        order.sort_by_key(|&i| self.summaries[i].feature(key));
        let sorted: Vec<Summary> = order.iter().map(|&i| self.summaries[i].clone()).collect();

        let mut parent: Vec<usize> = (0..n).collect();
        let link = |a: usize, b: usize, parent: &mut Vec<usize>| {
            if compare(self.probe(a), self.probe(b)).is_some() {
                let (ra, rb) = (find(parent, a), find(parent, b));
                parent[ra.max(rb)] = ra.min(rb);
            }
        };
        for (k, a) in sorted.iter().enumerate() {
            let limit = a.feature(key) as u16 + window as u16;
            for (m, b) in sorted.iter().enumerate().skip(k + 1).take_while(|(_, b)| b.feature(key) as u16 <= limit) {
                if a.within(b, window) {
                    link(order[k], order[m], &mut parent);
                }
            }
        }
        for (k, &i) in small.iter().enumerate() {
            for j in (0..n).filter(|j| !small[..=k].contains(j)) {
                link(i, j, &mut parent);
            }
        }

        let mut groups: HashMap<usize, Vec<Entry>> = HashMap::new();
        for i in 0..n {
            let root = find(&mut parent, i);
            groups.entry(root).or_default().push(self.items[i].entry.clone());
        }
        let mut out: Vec<Vec<Entry>> = groups.into_values().filter(|g| g.len() >= 2).collect();
        for g in &mut out {
            g.sort_by_key(|e| std::cmp::Reverse(e.modified));
        }
        out.sort_by(|a, b| b.len().cmp(&a.len()).then(b[0].modified.cmp(&a[0].modified)));
        out
    }

    fn most_varied_feature(&self, among: &[usize]) -> usize {
        let variance = |f: usize| {
            let values = among.iter().map(|&i| self.summaries[i].feature(f) as f64);
            let (sum, sq) = values.fold((0.0, 0.0), |(s, q), v| (s + v, q + v * v));
            let n = among.len().max(1) as f64;
            sq / n - (sum / n).powi(2)
        };
        (0..Summary::FEATURES).max_by(|&a, &b| variance(a).total_cmp(&variance(b))).unwrap_or(0)
    }
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}
