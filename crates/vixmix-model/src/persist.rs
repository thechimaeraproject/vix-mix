//! `setup.ron`: where it lives, how it is read and written, and the format version with its
//! migration seam. Writes are atomic and loads tolerant, through `chimaeraproject_fs`; the
//! directory is the user's configuration directory for `vix-mix`, resolved with the kit's
//! profile rule so a debug build never shares the installed app's setup.

use std::path::{Path, PathBuf};

use chimaeraproject_fs::app_dir::{Profile, resolve_from_env, user_config_base};
use chimaeraproject_fs::{FsError, Loaded, atomic_write, load_or_default};
use serde::Deserialize;

use crate::setup::Setup;

/// The application directory's name under the platform's per-user configuration directory.
pub const APP_DIR: &str = "vix-mix";

/// The setup's file name inside the profile's directory.
pub const FILE_NAME: &str = "setup.ron";

/// The format this build writes. A file at a lower version is migrated on load; a file at a
/// higher one was written by a newer vix-mix and is refused.
pub const VERSION: u32 = 1;

/// Why `setup.ron` could not be used. [`load`] hands it back inside
/// [`FsError::Parse`], whose message already ends with the fix (edit it, or delete it to
/// start from the default).
#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("it is not a vix-mix setup ({0})")]
    Ron(#[from] ron::error::SpannedError),
    #[error(
        "it was written by a newer vix-mix (format {found}; this build reads up to {supported}); run that vix-mix, or keep a copy of the file before this one saves over it"
    )]
    Newer { found: u32, supported: u32 },
    #[error("it says format {found}, which no vix-mix ever wrote")]
    Unknown { found: u32 },
}

/// Where the setup lives for this run: `user_config_base("vix-mix")` resolved with
/// `CHIMAERA_CONFIG_DIR` / `CHIMAERA_PROFILE`, a debug build in `profiles/dev`. Pass
/// `cfg!(debug_assertions)` from the binary. `None` when the platform names no per-user
/// configuration directory.
pub fn profile(debug_build: bool) -> Option<Profile> {
    user_config_base(APP_DIR).map(|base| resolve_from_env(base, debug_build))
}

/// The setup file inside `dir`.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// Reads `dir/setup.ron`. No file is the first start: [`Setup::default`] with no problem. A
/// file that cannot be read or parsed also gives the default, with the reason in
/// [`Loaded::problem`]; the caller must say so and must not save over the file unasked.
pub fn load(dir: &Path) -> Loaded<Setup> {
    load_or_default(path(dir), parse)
}

/// Writes `setup` to `dir/setup.ron` atomically. `dir` must exist.
pub fn save(dir: &Path, setup: &Setup) -> Result<(), FsError> {
    atomic_write(path(dir), to_ron(setup).as_bytes())
}

/// The text [`save`] writes: pretty RON without struct names, so the version can be read
/// before the rest.
pub fn to_ron(setup: &Setup) -> String {
    ron::ser::to_string_pretty(setup, ron::ser::PrettyConfig::default())
        // Every type in a setup is plain data with string keys; serializing cannot fail.
        .unwrap_or_default()
}

/// Parses `setup.ron` bytes, migrating an older format.
pub fn parse(bytes: &[u8]) -> Result<Setup, PersistError> {
    #[derive(Deserialize)]
    struct Header {
        version: u32,
    }
    let text = String::from_utf8_lossy(bytes);
    let header: Header = ron::from_str(&text)?;
    match header.version {
        VERSION => Ok(ron::from_str(&text)?),
        found if found > VERSION => Err(PersistError::Newer {
            found,
            supported: VERSION,
        }),
        // The migration seam: format N reads into its own types and converts to N + 1. There
        // is no older format yet.
        found => Err(PersistError::Unknown { found }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::example;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vixmix-model-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_setup_round_trips_through_the_file_byte_for_byte() {
        let dir = scratch("round-trip");
        let setup = example::setup();
        save(&dir, &setup).unwrap();
        let first = std::fs::read(path(&dir)).unwrap();
        let (loaded, problem) = load(&dir).into_parts();
        assert!(problem.is_none(), "{problem:?}");
        assert_eq!(loaded, setup);
        save(&dir, &loaded).unwrap();
        assert_eq!(std::fs::read(path(&dir)).unwrap(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_file_is_the_first_start_and_a_bad_one_says_why() {
        let dir = scratch("first-start");
        let (setup, problem) = load(&dir).into_parts();
        assert_eq!(setup, Setup::default());
        assert!(problem.is_none());

        std::fs::write(path(&dir), "(channels: oops").unwrap();
        let (setup, problem) = load(&dir).into_parts();
        assert_eq!(setup, Setup::default());
        let message = problem.expect("a reason").to_string();
        assert!(message.contains("setup.ron"), "{message}");
        assert!(message.contains("delete it"), "{message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_format_is_refused_rather_than_misread() {
        let setup = Setup {
            version: VERSION + 1,
            ..Setup::default()
        };
        let text = to_ron(&setup);
        let error = parse(text.as_bytes()).unwrap_err();
        assert!(matches!(error, PersistError::Newer { .. }));
        assert!(error.to_string().contains("newer vix-mix"));
    }

    #[test]
    fn the_directory_is_vix_mix_under_the_profile_rule() {
        let base = PathBuf::from("base").join(APP_DIR);
        let dev = chimaeraproject_fs::app_dir::resolve_with(base.clone(), None, None, true);
        assert_eq!(dev.dir, base.join("profiles").join("dev"));
        assert_eq!(path(&dev.dir), dev.dir.join("setup.ron"));
    }
}
