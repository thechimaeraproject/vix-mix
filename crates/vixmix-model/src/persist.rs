//! `setup.ron`: where it lives, how it is read and written, and the format version with its
//! migration seam. Writes are atomic and loads tolerant, through `chimaeraproject_fs`; the
//! directory is the user's configuration directory for `vix-mix`, resolved with the kit's
//! profile rule so a debug build never shares the installed app's setup.
//!
//! A file is read in three steps: its `version` first, then the whole file with the types of
//! that version (an older one converted forward, see `migrate.rs`), then the rules every
//! command keeps ([`Invalid`]). Whatever fails, [`load`] gives the default and the reason.

use std::path::{Path, PathBuf};

use chimaeraproject_fs::app_dir::{Profile, resolve_from_env, user_config_base};
use chimaeraproject_fs::{FsError, Loaded, atomic_write, load_or_default};
use serde::Deserialize;

pub use crate::check::Invalid;
use crate::check::check;
use crate::migrate;
use crate::setup::Setup;

/// The application directory's name under the platform's per-user configuration directory.
pub const APP_DIR: &str = "vix-mix";

/// The setup's file name inside the profile's directory.
pub const FILE_NAME: &str = "setup.ron";

/// The format this build writes. A file at a lower version is migrated on load; a file at a
/// higher one was written by a newer vix-mix and is refused. Format 2 added the id counters,
/// put `version` first and wrote a plugin's state as base64.
pub const VERSION: u32 = 2;

/// Why `setup.ron` could not be used. [`load`] hands it back inside
/// [`FsError::Parse`], whose message already ends with the fix (edit it, or delete it to
/// start from the default).
#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("it is not text ({0})")]
    NotText(#[from] std::str::Utf8Error),
    #[error("it is not a vix-mix setup ({0})")]
    Ron(#[from] ron::error::SpannedError),
    #[error(
        "it was written by a newer vix-mix (format {found}; this build reads up to {supported}); run that vix-mix, or keep a copy of the file before this one saves over it"
    )]
    Newer { found: u32, supported: u32 },
    #[error("it says format {found}, which no vix-mix ever wrote")]
    Unknown { found: u32 },
    #[error("it breaks a rule of the setup: {0}")]
    Invalid(#[from] Invalid),
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
/// file that cannot be read or used also gives the default, with the reason in
/// [`Loaded::problem`]; the caller must say so and must not save over the file unasked.
pub fn load(dir: &Path) -> Loaded<Setup> {
    load_or_default(path(dir), parse)
}

/// Writes `setup` to `dir/setup.ron` atomically. `dir` must exist.
pub fn save(dir: &Path, setup: &Setup) -> Result<(), FsError> {
    atomic_write(path(dir), to_ron(setup).as_bytes())
}

/// The text [`save`] writes: pretty RON without struct names, `version` on the first field's
/// line, LF line ends on every platform (so a file moved between Windows and Linux saves back
/// byte for byte), and a plugin's state as one base64 string.
pub fn to_ron(setup: &Setup) -> String {
    let pretty = ron::ser::PrettyConfig::new().new_line("\n");
    // A setup is plain data: strings, numbers, booleans, and plugin directories written
    // lossily as text. The serializer has nothing it could refuse.
    ron::ser::to_string_pretty(setup, pretty).unwrap_or_default()
}

/// Parses `setup.ron` bytes: the version, then the setup (migrating an older format), then
/// the rules every command keeps. The id counters are raised above every id in use, where a
/// hand edit left them behind.
pub fn parse(bytes: &[u8]) -> Result<Setup, PersistError> {
    #[derive(Deserialize)]
    struct Header {
        version: u32,
    }
    let text = std::str::from_utf8(bytes)?;
    let header: Header = ron::from_str(text)?;
    let mut setup = match header.version {
        VERSION => ron::from_str::<Setup>(text)?,
        found if found > VERSION => {
            return Err(PersistError::Newer {
                found,
                supported: VERSION,
            });
        }
        // The migration seam: each older format reads into its own types and converts
        // forward one step at a time (`migrate.rs`).
        1 => migrate::from_v1(text)?,
        found => return Err(PersistError::Unknown { found }),
    };
    setup.raise_counters();
    check(&setup)?;
    Ok(setup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::example;
    use crate::ids::{ChannelId, InstanceId};

    /// What the bootstrap's build wrote for the example console, with a plugin state, a
    /// quantum of 128 and a plugin directory: format 1, exactly.
    const V1: &str = include_str!("../tests/fixtures/setup-v1.ron");

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

    fn with_state() -> Setup {
        let mut setup = example::setup();
        setup.channels[0].chain[0].state = Some(vec![0, 1, 2, 127, 128, 254, 255]);
        setup.prefs.quantum = 128;
        setup.prefs.plugin_dirs = vec![PathBuf::from("/opt/vst3")];
        setup
    }

    #[test]
    fn a_setup_round_trips_through_the_file_byte_for_byte() {
        let dir = scratch("round-trip");
        let setup = with_state();
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
    fn the_file_reads_version_first_with_lf_and_the_state_on_one_line() {
        let text = to_ron(&with_state());
        assert!(text.starts_with("(\n    version: 2,\n"), "{text}");
        assert!(!text.contains('\r'));
        assert!(text.contains("state: Some(\"AAECf4D+/w==\")"), "{text}");
        assert!(text.contains("next_channel: (7)"), "{text}");
        assert!(text.contains("next_instance: (10)"), "{text}");
    }

    #[test]
    fn format_1_converts_forward_with_its_counters_and_its_state() {
        let setup = parse(V1.as_bytes()).unwrap();
        assert_eq!(setup, with_state());
        assert_eq!(setup.version, VERSION);
        // The bootstrap's file had no counters: they start above every id it holds.
        assert_eq!(setup.next_channel, ChannelId(7));
        assert_eq!(setup.next_instance, InstanceId(10));
        // Saved again, it is the current format, and reads back as it is.
        assert_eq!(parse(to_ron(&setup).as_bytes()).unwrap(), setup);
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

        std::fs::write(path(&dir), [b'(', 0xff, b')']).unwrap();
        let message = load(&dir).problem.expect("a reason").to_string();
        assert!(message.contains("not text"), "{message}");
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
        let never = to_ron(&Setup {
            version: 0,
            ..Setup::default()
        });
        assert!(matches!(
            parse(never.as_bytes()),
            Err(PersistError::Unknown { found: 0 })
        ));
    }

    #[test]
    fn a_file_that_breaks_a_rule_is_refused_with_the_rule_and_the_fix() {
        let dir = scratch("invalid");
        let mut setup = example::setup();
        setup.channels[1].id = setup.channels[0].id;
        save(&dir, &setup).unwrap();
        let (loaded, problem) = load(&dir).into_parts();
        assert_eq!(loaded, Setup::default());
        let message = problem.expect("a reason").to_string();
        assert!(message.contains("breaks a rule"), "{message}");
        assert!(
            message.contains("give each channel an id of its own"),
            "{message}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn counters_a_hand_edit_left_behind_are_raised_never_lowered() {
        let mut setup = example::setup();
        setup.next_channel = ChannelId(3);
        setup.next_instance = InstanceId(1);
        let read = parse(to_ron(&setup).as_bytes()).unwrap();
        assert_eq!(read.next_channel, ChannelId(7));
        assert_eq!(read.next_instance, InstanceId(10));
        setup.next_channel = ChannelId(40);
        let read = parse(to_ron(&setup).as_bytes()).unwrap();
        assert_eq!(read.next_channel, ChannelId(40));
    }

    #[test]
    fn the_directory_is_vix_mix_under_the_profile_rule() {
        let base = PathBuf::from("base").join(APP_DIR);
        let dev = chimaeraproject_fs::app_dir::resolve_with(base.clone(), None, None, true);
        assert_eq!(dev.dir, base.join("profiles").join("dev"));
        assert_eq!(path(&dev.dir), dev.dir.join("setup.ron"));
        let release = chimaeraproject_fs::app_dir::resolve_with(base.clone(), None, None, false);
        assert_eq!(release.dir, base);
    }
}
