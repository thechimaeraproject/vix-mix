//! The catalog the plugin browser searches: every VST3 effect a scan found, classified into
//! the prototype's types, with favourites and the recent list, cached as `plugins.ron` beside
//! the scan's per-bundle record (`crate::scan`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chimaeraproject_fs::{FsError, Loaded, atomic_write, load_or_default};
use serde::{Deserialize, Serialize};

use crate::scan::CachedBundle;

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

impl Catalog {
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

    /// The browser's Maker list: every vendor with how many plugins it has, by name.
    pub fn vendors(&self) -> Vec<(&str, usize)> {
        let mut vendors: Vec<(&str, usize)> = Vec::new();
        for plugin in &self.plugins {
            match vendors.iter_mut().find(|(v, _)| *v == plugin.vendor) {
                Some((_, count)) => *count += 1,
                None => vendors.push((&plugin.vendor, 1)),
            }
        }
        vendors.sort_by_key(|(v, _)| v.to_lowercase());
        vendors
    }

    /// Reads the catalog in `dir/plugins.ron`; no file is an empty catalog (scan to fill it).
    pub fn load(dir: &Path) -> Loaded<Catalog> {
        let (file, problem) = load_file(dir).into_parts();
        Loaded {
            value: Catalog {
                plugins: file.plugins,
                favorites: file.favorites,
                recent: file.recent,
            },
            problem,
        }
    }

    /// Writes the catalog to `dir/plugins.ron` atomically, keeping the scan's per-bundle
    /// record already in the file (a scan saves its own with
    /// [`ScanOutcome::save`](crate::ScanOutcome::save)). `dir` must exist.
    pub fn save(&self, dir: &Path) -> Result<(), FsError> {
        write_file(dir, self, &read_file(dir).bundles)
    }
}

/// `plugins.ron`: the catalog's three lists and the scan's per-bundle record. Every field
/// defaults, so a file written before the record existed still loads.
#[derive(Default, Deserialize)]
pub(crate) struct CacheFile {
    #[serde(default)]
    plugins: Vec<PluginEntry>,
    #[serde(default)]
    favorites: BTreeSet<String>,
    #[serde(default)]
    recent: Vec<String>,
    #[serde(default)]
    pub(crate) bundles: Vec<CachedBundle>,
}

/// The same shape, borrowed, for writing.
#[derive(Serialize)]
struct CacheFileRef<'a> {
    plugins: &'a [PluginEntry],
    favorites: &'a BTreeSet<String>,
    recent: &'a [String],
    bundles: &'a [CachedBundle],
}

fn load_file(dir: &Path) -> Loaded<CacheFile> {
    load_or_default(dir.join(FILE_NAME), |bytes| {
        ron::from_str::<CacheFile>(&String::from_utf8_lossy(bytes))
    })
}

/// `dir/plugins.ron`, or an empty one when it is missing or unreadable.
pub(crate) fn read_file(dir: &Path) -> CacheFile {
    load_file(dir).into_parts().0
}

/// Writes `dir/plugins.ron` atomically. A catalog that does not serialize (a path that is not
/// UTF-8 in an entry the scan did not make) is refused rather than written as an empty file.
pub(crate) fn write_file(
    dir: &Path,
    catalog: &Catalog,
    bundles: &[CachedBundle],
) -> Result<(), FsError> {
    let file = CacheFileRef {
        plugins: &catalog.plugins,
        favorites: &catalog.favorites,
        recent: &catalog.recent,
        bundles,
    };
    let path = dir.join(FILE_NAME);
    let text =
        ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).map_err(|error| {
            FsError::Write {
                path: path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, error),
            }
        })?;
    atomic_write(path, text.as_bytes())
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

    #[test]
    fn the_maker_list_counts_each_vendor_by_name() {
        assert_eq!(
            catalog().vendors(),
            [
                ("FabFilter", 2),
                ("Nobody", 1),
                ("Prolific", 1),
                ("Tokyo Dawn Labs", 1),
                ("Xfer Records", 1)
            ]
        );
    }

    #[test]
    fn saving_the_catalog_keeps_the_scans_record_and_an_older_file_still_loads() {
        let dir =
            std::env::temp_dir().join(format!("vixmix-plugins-record-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bundles = vec![CachedBundle {
            path: PathBuf::from("OTT.vst3"),
            modified: Some((1_700_000_000, 250)),
            entries: vec![entry("OTT", "Xfer Records", PluginKind::Dynamics)],
            failure: None,
        }];
        let mut catalog = catalog();
        write_file(&dir, &catalog, &bundles).unwrap();
        // A star saved later must not lose the record a rescan needs.
        catalog.set_favorite("vst3:OTT", true);
        catalog.save(&dir).unwrap();
        assert_eq!(read_file(&dir).bundles, bundles);
        let (loaded, problem) = Catalog::load(&dir).into_parts();
        assert!(problem.is_none(), "{problem:?}");
        assert_eq!(loaded, catalog);

        // The catalog alone, as lane 00 wrote it: no record, nothing lost.
        let older =
            ron::ser::to_string_pretty(&catalog, ron::ser::PrettyConfig::default()).unwrap();
        std::fs::write(dir.join(FILE_NAME), older).unwrap();
        let (loaded, problem) = Catalog::load(&dir).into_parts();
        assert!(problem.is_none(), "{problem:?}");
        assert_eq!(loaded, catalog);
        assert!(read_file(&dir).bundles.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
