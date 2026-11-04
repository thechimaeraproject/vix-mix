# Kit filesystem helpers adoption

**Adopted 2026-11-04 by the bootstrap ([handoff 00](handoffs/00-bootstrap.md)) and completed
the same day by the model ([handoff 01](handoffs/01-model.md)), at the kit's release tag
`kit-source.json` pins.** Vix Mix takes `chimaeraproject-fs` (imported as
`chimaeraproject_fs`) for every file it keeps, instead of writing its own. The kit's
[fs reference](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/fs.md)
defines what the crate guarantees; this record says which Vix Mix sites take it, what stays
here, and what changed observably. It is a git dependency on the kit's release tag in the
root `Cargo.toml`'s *The kit* group, at the same tag as the other three kit crates, moved by
`kit bump` with `kit-source.json` and `Cargo.lock` ([kit-cli.md](kit-cli.md)).

## What Vix Mix takes

- **`atomic_write`** for `setup.ron` (`vixmix_model::persist::save`) and `plugins.ron`
  (`vixmix_plugins::Catalog::save`): a reader, or the next start after a crash, sees the old
  file or the new one, never a torn mix. `persist::save` hands it the whole text
  `persist::to_ron` makes, which cannot fail, so a save never writes a partial or empty setup.
- **`load_or_default`** for both (`persist::load`, `Catalog::load`): no file is the first
  start; a file that cannot be read or used gives the default *and the reason*, which the host
  shows as a toast. For `setup.ron` the parser is `persist::parse`, and the reason is the
  kit's `FsError::Parse` around a `PersistError`: not text, not RON, a newer format, a format
  no vix-mix wrote, or a rule of the setup broken by a hand edit (`persist::Invalid`, which
  names the rule and the edit that fixes it). An older format is not a reason: it is migrated.
- **`app_dir::user_config_base("vix-mix")` and `resolve_from_env`** in
  `vixmix_model::persist::profile`: `%APPDATA%\vix-mix` on Windows, `$XDG_CONFIG_HOME/vix-mix`
  or `~/.config/vix-mix` on Linux; a debug build in `profiles/dev`, and `CHIMAERA_CONFIG_DIR` /
  `CHIMAERA_PROFILE` honoured, so a development build never touches the installed app's setup.
  `cfg!(debug_assertions)` is passed from the binary.

## What stays in Vix Mix

- **The formats and names**: RON, `setup.ron` and `plugins.ron`, the `vix-mix` directory, the
  format `version` and the migrations from every older format, the rules a loaded setup must
  keep, LF line ends and plugin state as base64 ([reference/model.md](reference/model.md)
  § Persistence).
- **Creating the directory** before the first save (the kit never creates a parent), and
  **what to do with a file that did not load**: `RealHost` moves an unreadable `setup.ron` aside
  to `setup.ron.unreadable` before saving, rather than replacing the user's file with the
  default the load fell back to.

## What changed observably

Nothing a user could compare with: Vix Mix had no filesystem code before. A first start writes
nothing until the setup is saved; a hand edit that stops parsing, or that breaks a rule of the
setup, is reported, kept aside, and replaced only by a deliberate save. A `setup.ron` the
bootstrap's build wrote (format 1) opens and is written back in the current format at the next
save.

## How you know it worked

`cargo test -p vixmix-model` (`persist`: a byte-for-byte round trip, the format-1 fixture
converted forward, the first start, a bad file's reason, a newer format refused, a broken rule
refused with its fix, the profile rule; `tests/applied.rs`: every setup the commands make
round-trips through the file) and `cargo test -p vixmix-host` (an unreadable setup kept aside).
