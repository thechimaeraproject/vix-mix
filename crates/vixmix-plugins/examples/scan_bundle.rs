//! The scan's child process for this crate's tests, standing in for `vix-mix --scan-bundle`:
//! `answer_scan` on the bundle named by its first argument, exiting with its code. It prints
//! noise to stdout around the answer, a line before it left without its end, as a plugin or
//! Wine would, so every scan the tests run through it proves the answer survives that.
//! Built by `cargo test`; `tests/common/mod.rs` finds it.

use std::io::Write;
use std::path::PathBuf;

use chimaeraproject_vst3_host::HostContext;

fn main() {
    let Some(bundle) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("scan_bundle: name the bundle to describe, e.g. `scan_bundle Echo.vst3`");
        std::process::exit(2);
    };
    let mut stdout = std::io::stdout();
    let _ = write!(
        stdout,
        "noise before the answer\na line left without its end"
    );
    let _ = stdout.flush();
    let code = vixmix_plugins::answer_scan(&bundle, &HostContext::new("Vix Mix Tests"));
    let _ = writeln!(stdout, "noise after the answer");
    std::process::exit(code);
}
