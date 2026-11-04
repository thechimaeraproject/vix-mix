//! The catalog the plugin browser searches: every VST3 effect a scan found, classified into
//! the prototype's types, with favourites and the recent list, cached as `plugins.ron`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chimaeraproject_fs::{FsError, Loaded, atomic_write, load_or_default};
use chimaeraproject_vst3_host::{HostContext, ScanOptions, scan};
use serde::{Deserialize, Serialize};

/// The cache's file name in the profile's directory, beside `setup.ron`.
pub const FILE_NAME: &str = "plugins.ron";

/// How many plugins the recent list keeps.
pub const RECENT_LIMIT: usize = 8;

/// The browser's Type list, in the prototype's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PluginKind {
    /// "EQ".
    Eq,
    Dynamics,
    Reverb,
    Delay,
    Distortion,
    Modulation,
    /// "Pitch & vocal".
    PitchVocal,
    Restoration,
    /// "Multi-effect": anything no other type claims.
    MultiEffect,
    /// "Utility": tools and analyzers.
    Utility,
}

impl PluginKind {
    /// Every kind, in the browser's order.
    pub const ALL: [PluginKind; 10] = [
        PluginKind::Eq,
        PluginKind::Dynamics,
        PluginKind::Reverb,
        PluginKind::Delay,
        PluginKind::Distortion,
        PluginKind::Modulation,
        PluginKind::PitchVocal,
        PluginKind::Restoration,
        PluginKind::MultiEffect,
        PluginKind::Utility,
    ];

    /// What the browser shows.
    pub fn label(self) -> &'static str {
        match self {
            PluginKind::Eq => "EQ",
            PluginKind::Dynamics => "Dynamics",
            PluginKind::Reverb => "Reverb",
            PluginKind::Delay => "Delay",
            PluginKind::Distortion => "Distortion",
            PluginKind::Modulation => "Modulation",
            PluginKind::PitchVocal => "Pitch & vocal",
            PluginKind::Restoration => "Restoration",
            PluginKind::MultiEffect => "Multi-effect",
            PluginKind::Utility => "Utility",
        }
    }

    /// The kind whose label is `label` (the prototype's category strings).
    pub fn from_label(label: &str) -> Option<PluginKind> {
        PluginKind::ALL.into_iter().find(|k| k.label() == label)
    }

    /// The kind a plugin's VST3 sub-categories (`Fx|EQ`, `Fx|Pitch Shift`, …) say, read in
    /// the plugin's own order so its first meaningful one wins; `Fx` itself says nothing.
    /// Nothing recognised is a multi-effect.
    pub fn from_sub_categories<S: AsRef<str>>(sub_categories: &[S]) -> PluginKind {
        sub_categories
            .iter()
            .flat_map(|s| s.as_ref().split('|'))
            .find_map(|part| {
                let part = part.trim().to_ascii_lowercase();
                Some(match part.as_str() {
                    "dynamics" => PluginKind::Dynamics,
                    "eq" => PluginKind::Eq,
                    "reverb" => PluginKind::Reverb,
                    "delay" => PluginKind::Delay,
                    "distortion" => PluginKind::Distortion,
                    "modulation" => PluginKind::Modulation,
                    "pitch shift" => PluginKind::PitchVocal,
                    p if p.contains("vocal") => PluginKind::PitchVocal,
                    "restoration" => PluginKind::Restoration,
                    "tools" | "analyzer" => PluginKind::Utility,
                    _ => return None,
                })
            })
            .unwrap_or(PluginKind::MultiEffect)
    }
}

/// One effect the browser lists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginEntry {
    /// `vst3:` and the class id's hex: what a setup's `PluginRef` stores.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub kind: PluginKind,
    /// The bundle's file name, what a setup stores beside the id.
    pub bundle: String,
    /// Where the bundle was found by the last scan.
    pub path: PathBuf,
}

/// What the browser's left column selects when the search field is empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Favorites,
    Recent,
    Kind(PluginKind),
    Vendor(String),
}

/// Every effect found, plus the user's favourites and recent picks (both by id).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub plugins: Vec<PluginEntry>,
    pub favorites: BTreeSet<String>,
    /// Newest first, at most [`RECENT_LIMIT`].
    pub recent: Vec<String>,
}

/// What a scan produced: the catalog, and every bundle that could not be described, each a
/// message ending with what to do.
#[derive(Debug, Default)]
pub struct ScanOutcome {
    pub catalog: Catalog,
    pub failures: Vec<String>,
}

impl Catalog {
    /// Scans `dirs` (the SDK's places from `default_search_dirs()` plus the preferences'
    /// extras) for VST3 effects, on the UI thread. Favourites and recent are empty: the caller
    /// carries them over from the catalog it had.
    pub fn scan(dirs: &[PathBuf], host: &HostContext) -> ScanOutcome {
        let report = scan(dirs, host, &ScanOptions::default());
        let mut plugins = Vec::new();
        for bundle in &report.bundles {
            for class in bundle.audio_classes() {
                let is_effect = class
                    .sub_categories
                    .iter()
                    .flat_map(|s| s.split('|'))
                    .any(|part| part.trim() == "Fx");
                if !is_effect {
                    continue;
                }
                plugins.push(PluginEntry {
                    id: class.id.processor_id(),
                    name: class.name.clone(),
                    vendor: class.vendor.clone(),
                    kind: PluginKind::from_sub_categories(&class.sub_categories),
                    bundle: bundle.name.clone(),
                    path: bundle.path.clone(),
                });
            }
        }
        plugins.sort_by_key(|p| p.name.to_lowercase());
        let failures = report
            .failures
            .iter()
            .map(|f| format!("{}: {}", f.path.display(), f.error))
            .collect();
        ScanOutcome {
            catalog: Catalog {
                plugins,
                ..Catalog::default()
            },
            failures,
        }
    }

    /// The entry with `id`.
    pub fn get(&self, id: &str) -> Option<&PluginEntry> {
        self.plugins.iter().find(|p| p.id == id)
    }

    /// The browser's list. A query searches everything, whatever the filter, ranked as the
    /// prototype ranks it: name starts with the query, then name contains it, then vendor,
    /// then type, each group by name. With no query, the filter's plugins by name (recent in
    /// its own order).
    pub fn search(&self, query: &str, filter: Filter) -> Vec<&PluginEntry> {
        let by_name =
            |a: &&PluginEntry, b: &&PluginEntry| a.name.to_lowercase().cmp(&b.name.to_lowercase());
        let q = query.trim().to_lowercase();
        if !q.is_empty() {
            let score = |p: &PluginEntry| {
                let name = p.name.to_lowercase();
                if name.starts_with(&q) {
                    Some(0)
                } else if name.contains(&q) {
                    Some(1)
                } else if p.vendor.to_lowercase().contains(&q) {
                    Some(2)
                } else if p.kind.label().to_lowercase().contains(&q) {
                    Some(3)
                } else {
                    None
                }
            };
            let mut hits: Vec<(u8, &PluginEntry)> = self
                .plugins
                .iter()
                .filter_map(|p| score(p).map(|s| (s, p)))
                .collect();
            hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| by_name(&a.1, &b.1)));
            return hits.into_iter().map(|(_, p)| p).collect();
        }
        let mut list: Vec<&PluginEntry> = match &filter {
            Filter::All => self.plugins.iter().collect(),
            Filter::Favorites => self
                .plugins
                .iter()
                .filter(|p| self.favorites.contains(&p.id))
                .collect(),
            Filter::Recent => {
                return self.recent.iter().filter_map(|id| self.get(id)).collect();
            }
            Filter::Kind(kind) => self.plugins.iter().filter(|p| p.kind == *kind).collect(),
            Filter::Vendor(vendor) => self
                .plugins
                .iter()
                .filter(|p| &p.vendor == vendor)
                .collect(),
        };
        list.sort_by(by_name);
        list
    }

    /// Puts `id` at the front of the recent list, once, keeping [`RECENT_LIMIT`].
    pub fn used(&mut self, id: &str) {
        self.recent.retain(|r| r != id);
        self.recent.insert(0, id.to_string());
        self.recent.truncate(RECENT_LIMIT);
    }

    /// Stars or unstars `id`.
    pub fn set_favorite(&mut self, id: &str, favorite: bool) {
        if favorite {
            self.favorites.insert(id.to_string());
        } else {
            self.favorites.remove(id);
        }
    }

    /// Reads `dir/plugins.ron`; no file is an empty catalog (scan to fill it).
    pub fn load(dir: &Path) -> Loaded<Catalog> {
        load_or_default(dir.join(FILE_NAME), |bytes| {
            ron::from_str::<Catalog>(&String::from_utf8_lossy(bytes))
        })
    }

    /// Writes `dir/plugins.ron` atomically. `dir` must exist.
    pub fn save(&self, dir: &Path) -> Result<(), FsError> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            // Plain data with string keys; serializing cannot fail.
            .unwrap_or_default();
        atomic_write(dir.join(FILE_NAME), text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, vendor: &str, kind: PluginKind) -> PluginEntry {
        PluginEntry {
            id: format!("vst3:{name}"),
            name: name.to_string(),
            vendor: vendor.to_string(),
            kind,
            bundle: format!("{name}.vst3"),
            path: PathBuf::from(format!("{name}.vst3")),
        }
    }

    fn catalog() -> Catalog {
        Catalog {
            plugins: vec![
                entry("Pro-Q 3", "FabFilter", PluginKind::Eq),
                entry("Pro-L 2", "FabFilter", PluginKind::Dynamics),
                entry("TDR Nova", "Tokyo Dawn Labs", PluginKind::Eq),
                entry("OTT", "Xfer Records", PluginKind::Dynamics),
                entry("Supro", "Nobody", PluginKind::Distortion),
                entry("Room", "Prolific", PluginKind::Reverb),
            ],
            ..Catalog::default()
        }
    }

    fn names(list: Vec<&PluginEntry>) -> Vec<&str> {
        list.into_iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn sub_categories_classify_in_the_plugins_own_order() {
        assert_eq!(PluginKind::from_sub_categories(&["Fx|EQ"]), PluginKind::Eq);
        assert_eq!(
            PluginKind::from_sub_categories(&["Fx", "Pitch Shift"]),
            PluginKind::PitchVocal
        );
        assert_eq!(
            PluginKind::from_sub_categories(&["Fx|Vocals|Dynamics"]),
            PluginKind::PitchVocal
        );
        assert_eq!(
            PluginKind::from_sub_categories(&["Fx|Analyzer"]),
            PluginKind::Utility
        );
        assert_eq!(
            PluginKind::from_sub_categories(&["Fx|Spatial"]),
            PluginKind::MultiEffect
        );
        assert_eq!(
            PluginKind::from_sub_categories::<&str>(&[]),
            PluginKind::MultiEffect
        );
        for kind in PluginKind::ALL {
            assert_eq!(PluginKind::from_label(kind.label()), Some(kind));
        }
    }

    #[test]
    fn a_query_ranks_name_start_then_name_then_vendor_then_type() {
        let catalog = catalog();
        // "pro": two names start with it, one contains it, one vendor contains it.
        assert_eq!(
            names(catalog.search("pro", Filter::Kind(PluginKind::Reverb))),
            ["Pro-L 2", "Pro-Q 3", "Supro", "Room"]
        );
        assert_eq!(
            names(catalog.search(" dyn ", Filter::All)),
            ["OTT", "Pro-L 2"]
        );
        assert!(catalog.search("zzz", Filter::All).is_empty());
    }

    #[test]
    fn filters_list_by_name_and_recent_keeps_its_own_order() {
        let mut catalog = catalog();
        assert_eq!(
            names(catalog.search("", Filter::Kind(PluginKind::Eq))),
            ["Pro-Q 3", "TDR Nova"]
        );
        catalog.set_favorite("vst3:OTT", true);
        catalog.set_favorite("vst3:Pro-Q 3", true);
        assert_eq!(
            names(catalog.search("", Filter::Favorites)),
            ["OTT", "Pro-Q 3"]
        );
        for _ in 0..2 {
            for p in ["Room", "OTT", "Supro"] {
                catalog.used(&format!("vst3:{p}"));
            }
        }
        assert_eq!(
            names(catalog.search("", Filter::Recent)),
            ["Supro", "OTT", "Room"]
        );
        for i in 0..20 {
            catalog.used(&format!("vst3:gone{i}"));
        }
        assert_eq!(catalog.recent.len(), RECENT_LIMIT);
    }

    #[test]
    fn favorites_and_recent_survive_the_cache_file() {
        let dir = std::env::temp_dir().join(format!("vixmix-plugins-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut catalog = catalog();
        catalog.set_favorite("vst3:TDR Nova", true);
        catalog.used("vst3:OTT");
        catalog.save(&dir).unwrap();
        let (loaded, problem) = Catalog::load(&dir).into_parts();
        assert!(problem.is_none(), "{problem:?}");
        assert_eq!(loaded, catalog);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
