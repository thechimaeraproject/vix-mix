//! Scanning: where the catalog looks for VST3 effects, and the per-bundle cache that keeps a
//! rescan from describing a bundle that has not changed.
//!
//! The kit's `scan_bundle` describes one bundle, reading its `moduleinfo.json` or, without
//! one, loading it and asking its factory: the slow part of a scan, and the part that runs a
//! plugin's code. So each bundle the scan describes is remembered in `plugins.ron` with its
//! modification time and what it gave (its effects, or why it failed), and a rescan reuses
//! that for a bundle whose time has not moved.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chimaeraproject_fs::FsError;
use chimaeraproject_vst3_host::{
    HostContext, ScanOptions, ScannedBundle, default_search_dirs, resolve_binary, scan_bundle,
};
use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, PluginEntry, PluginKind, write_file};

/// Where to scan: the SDK's standard places (`default_search_dirs()`), then the preferences'
/// extra directories (`Prefs::plugin_dirs`), each once.
pub fn search_dirs(extras: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = default_search_dirs();
    for extra in extras {
        if !dirs.contains(extra) {
            dirs.push(extra.clone());
        }
    }
    dirs
}

/// What one scan remembered of one bundle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct CachedBundle {
    pub(crate) path: PathBuf,
    /// The newest modification time of the bundle, its binary for this platform and its
    /// `moduleinfo.json`, as seconds and nanoseconds since the Unix epoch; `None` when none
    /// could be read (or it is before the epoch), which is never reused.
    pub(crate) modified: Option<(u64, u32)>,
    /// The effects it exported.
    pub(crate) entries: Vec<PluginEntry>,
    /// Why it could not be described, said again while it is unchanged.
    pub(crate) failure: Option<String>,
}

/// The per-bundle record of the last scan, kept in `plugins.ron` beside the catalog.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScanCache {
    pub(crate) bundles: Vec<CachedBundle>,
}

impl ScanCache {
    /// The record in `dir/plugins.ron`; empty (every bundle described afresh) when there is
    /// none or it cannot be read.
    pub fn load(dir: &Path) -> ScanCache {
        ScanCache {
            bundles: crate::catalog::read_file(dir).bundles,
        }
    }

    /// Bundles remembered.
    pub fn len(&self) -> usize {
        self.bundles.len()
    }

    /// Whether nothing is remembered.
    pub fn is_empty(&self) -> bool {
        self.bundles.is_empty()
    }

    /// Forgets every bundle that failed, so the next [`rescan`](Catalog::rescan) describes
    /// them again and reuses only the bundles that worked. The menu's Rescan plugins calls it
    /// first: a bundle can fail for a reason outside it that its time does not show (a
    /// runtime library installed later, a file locked during the scan), and a failure kept
    /// would repeat a fix the user has already made. A start does not, so a broken plugin is
    /// not loaded at every start.
    pub fn forget_failures(&mut self) {
        self.bundles.retain(|bundle| bundle.failure.is_none());
    }

    fn get(&self, path: &Path) -> Option<&CachedBundle> {
        self.bundles.iter().find(|b| b.path == path)
    }
}

/// What a scan produced: the catalog, every bundle that could not be described (each a
/// message ending with what to do), the cache for the next rescan, and how many bundles were
/// described afresh or reused unchanged.
#[derive(Debug, Default)]
pub struct ScanOutcome {
    pub catalog: Catalog,
    pub failures: Vec<String>,
    pub cache: ScanCache,
    pub described: usize,
    pub reused: usize,
}

impl ScanOutcome {
    /// Writes the catalog and the scan's cache to `dir/plugins.ron` atomically. `dir` must
    /// exist.
    pub fn save(&self, dir: &Path) -> Result<(), FsError> {
        write_file(dir, &self.catalog, &self.cache.bundles)
    }
}

impl Catalog {
    /// Scans `dirs` (see [`search_dirs`]) for VST3 effects on the UI thread, describing every
    /// bundle. Favourites and recent are empty: [`rescan`](Catalog::rescan) carries them over.
    pub fn scan(dirs: &[PathBuf], host: &HostContext) -> ScanOutcome {
        Catalog::default().rescan(dirs, host, &ScanCache::default())
    }

    /// Scans `dirs` again on the UI thread, describing only the bundles that are new or whose
    /// modification time moved since `cache` was made; an unchanged bundle gives what it gave
    /// last time, a failure included (see [`ScanCache::forget_failures`]). Audio classes whose sub-categories say `Fx` are kept,
    /// one entry per class (the first bundle in path order), sorted by name. Favourites and
    /// recent are carried over from `self`.
    pub fn rescan(&self, dirs: &[PathBuf], host: &HostContext, cache: &ScanCache) -> ScanOutcome {
        let mut outcome = ScanOutcome::default();
        for path in find_bundles(dirs) {
            // Everything cached is written to plugins.ron as text: a path that is not UTF-8
            // cannot be, so it is reported and never cached.
            if path.to_str().is_none() {
                outcome.failures.push(format!(
                    "{}: its path is not valid UTF-8, so it cannot be cached; rename the bundle or the folder it is in, then rescan",
                    path.display()
                ));
                continue;
            }
            let modified = modified(&path);
            let unchanged = cache
                .get(&path)
                .filter(|cached| modified.is_some() && cached.modified == modified);
            let bundle = match unchanged {
                Some(cached) => {
                    outcome.reused += 1;
                    cached.clone()
                }
                None => {
                    outcome.described += 1;
                    describe(path, modified, host)
                }
            };
            outcome.failures.extend(bundle.failure.iter().cloned());
            outcome.cache.bundles.push(bundle);
        }
        let mut seen = HashSet::new();
        let mut plugins: Vec<PluginEntry> = outcome
            .cache
            .bundles
            .iter()
            .flat_map(|b| b.entries.iter())
            .filter(|p| seen.insert(p.id.clone()))
            .cloned()
            .collect();
        plugins.sort_by_key(|p| p.name.to_lowercase());
        outcome.catalog = Catalog {
            plugins,
            favorites: self.favorites.clone(),
            recent: self.recent.clone(),
        };
        outcome
    }
}

/// Describes one bundle through the kit: its effects, or why it could not be read. Warnings
/// that did not stop it are logged here, on the UI thread.
fn describe(path: PathBuf, modified: Option<(u64, u32)>, host: &HostContext) -> CachedBundle {
    match scan_bundle(&path, host, &ScanOptions::default()) {
        Ok(bundle) => {
            for warning in &bundle.warnings {
                log::warn!("{}: {warning}", path.display());
            }
            CachedBundle {
                entries: effects_of(&bundle),
                path,
                modified,
                failure: None,
            }
        }
        Err(error) => CachedBundle {
            failure: Some(format!("{}: {error}", path.display())),
            path,
            modified,
            entries: Vec::new(),
        },
    }
}

/// The effects a described bundle exports, as the browser lists them.
fn effects_of(bundle: &ScannedBundle) -> Vec<PluginEntry> {
    bundle
        .audio_classes()
        .filter(|class| class.is_effect())
        .map(|class| PluginEntry {
            id: class.id.processor_id(),
            name: class.name.clone(),
            vendor: class.vendor.clone(),
            kind: PluginKind::from_sub_categories(&class.sub_categories),
            bundle: bundle.name.clone(),
            path: bundle.path.clone(),
        })
        .collect()
}

/// The newest modification time among the bundle, its binary for this platform and its
/// `moduleinfo.json`, since the Unix epoch: an update that rewrites only the binary inside the
/// bundle still moves it.
fn modified(bundle: &Path) -> Option<(u64, u32)> {
    let moduleinfo = bundle
        .join("Contents")
        .join("Resources")
        .join("moduleinfo.json");
    [
        Some(bundle.to_path_buf()),
        resolve_binary(bundle).ok(),
        Some(moduleinfo),
    ]
    .into_iter()
    .flatten()
    .filter_map(|path| std::fs::metadata(path).and_then(|m| m.modified()).ok())
    .max()
    .and_then(|newest| newest.duration_since(UNIX_EPOCH).ok())
    .map(|since| (since.as_secs(), since.subsec_nanos()))
}

fn is_bundle_name(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("vst3"))
}

/// Every `.vst3` under `dirs`, as the kit's `scan` walks them: a directory named in `dirs`
/// that is itself a bundle counts, ordinary subdirectories are descended into, a bundle never
/// is, each directory's entries in name order. A bundle reached twice is listed once.
fn find_bundles(dirs: &[PathBuf]) -> Vec<PathBuf> {
    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if is_bundle_name(&path) {
                out.push(path);
            } else if path.is_dir() {
                collect(&path, out);
            }
        }
    }
    let mut found = Vec::new();
    for dir in dirs {
        if is_bundle_name(dir) {
            found.push(dir.clone());
        } else {
            collect(dir, &mut found);
        }
    }
    let mut seen = HashSet::new();
    found.retain(|path| seen.insert(path.clone()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vixmix-plugins-scan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_preferences_extras_follow_the_standard_places_once_each() {
        let standard = default_search_dirs();
        let extra = PathBuf::from("/opt/vst3-extra");
        let mut extras = vec![extra.clone(), extra.clone()];
        extras.extend(standard.first().cloned());
        let dirs = search_dirs(&extras);
        assert_eq!(&dirs[..standard.len()], &standard[..]);
        assert_eq!(&dirs[standard.len()..], [extra]);
    }

    #[test]
    fn bundles_are_found_as_the_kit_walks_and_listed_once() {
        let root = scratch("walk");
        let b = root.join("B.vst3");
        let c = root.join("sub").join("C.VST3");
        for dir in [b.join("Contents").join("Inner.vst3"), c.clone()] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(root.join("notes.txt"), "not a plugin").unwrap();
        let found = find_bundles(&[root.clone(), root.join("sub"), b.clone()]);
        assert_eq!(found, [b, c], "never inside a bundle, never twice");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unchanged_bundle_is_not_described_again_and_a_changed_one_is() {
        let root = scratch("cache");
        let broken = root.join("Broken.vst3");
        std::fs::create_dir_all(&broken).unwrap();
        let host = HostContext::new("Vix Mix Tests");

        let first = Catalog::scan(std::slice::from_ref(&root), &host);
        assert_eq!((first.described, first.reused), (1, 0));
        assert_eq!(first.failures.len(), 1, "{:?}", first.failures);
        assert!(first.failures[0].contains("Broken.vst3"));
        assert!(first.catalog.plugins.is_empty());

        let mut catalog = first.catalog.clone();
        catalog.set_favorite("vst3:kept", true);
        let again = catalog.rescan(std::slice::from_ref(&root), &host, &first.cache);
        assert_eq!((again.described, again.reused), (0, 1));
        assert_eq!(again.failures, first.failures, "said again while unchanged");
        assert!(again.catalog.favorites.contains("vst3:kept"));

        // The bundle changes: a moduleinfo.json appears, written later than anything before.
        let resources = broken.join("Contents").join("Resources");
        std::fs::create_dir_all(&resources).unwrap();
        let info = resources.join("moduleinfo.json");
        std::fs::write(&info, "{ not json").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&info)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(3600))
            .unwrap();
        let changed = again
            .catalog
            .rescan(std::slice::from_ref(&root), &host, &again.cache);
        assert_eq!((changed.described, changed.reused), (1, 0));
        let _ = std::fs::remove_dir_all(&root);
    }
}
