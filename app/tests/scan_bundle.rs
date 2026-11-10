//! `vix-mix --scan-bundle <bundle>`, the child process the plugin scan runs for each bundle it
//! must load: the answer on one marked line of stdout and exit 0, for a bundle it describes
//! and for one that fails; the flag without a bundle is refused with exit 2. The bundle
//! described here carries a `moduleinfo.json` and no binary, because `cargo test -p vix-mix`
//! does not build the plugins crate's test plugin; loading a plugin in the child, and one that
//! aborts there, are `vixmix-plugins`' tests (`tests/scan_child.rs`), through the same
//! `answer_scan`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use vixmix_plugins::ANSWER_MARK;

const CLASS: &str = "1D4E7A205B3C4F819E62D0A73F18C5B9";

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("vix-mix-scan-bundle-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn vix_mix() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vix-mix"))
}

fn scan_bundle(bundle: &Path) -> Output {
    vix_mix().arg("--scan-bundle").arg(bundle).output().unwrap()
}

/// The one answer on `output`'s stdout.
fn answer(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let answers: Vec<&str> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix(ANSWER_MARK))
        .collect();
    let [answer] = answers.as_slice() else {
        panic!("one answer: {stdout}");
    };
    answer.to_string()
}

#[test]
fn a_bundle_is_described_on_one_marked_line_and_the_child_exits_0() {
    let bundle = scratch("described").join("Info Echo.vst3");
    let resources = bundle.join("Contents").join("Resources");
    std::fs::create_dir_all(&resources).unwrap();
    std::fs::write(
        resources.join("moduleinfo.json"),
        format!(
            r#"{{"Name": "Info Echo", "Classes": [{{"CID": "{CLASS}", "Category": "Audio Module Class",
                "Name": "Vix Mix Info Echo", "Vendor": "chimaeraproject", "Sub Categories": "Fx|Delay"}}]}}"#
        ),
    )
    .unwrap();
    let output = scan_bundle(&bundle);
    assert!(output.status.success(), "{output:?}");
    let answer = answer(&output);
    assert!(answer.contains("Vix Mix Info Echo"), "{answer}");
    assert!(answer.contains(&format!("vst3:{CLASS}")), "{answer}");
    assert!(answer.contains("failure:None"), "{answer}");
}

#[test]
fn a_path_that_is_not_a_bundle_is_answered_with_its_failure_and_the_child_exits_0() {
    let output = scan_bundle(&scratch("missing").join("Nothing.vst3"));
    assert!(output.status.success(), "{output:?}");
    let answer = answer(&output);
    assert!(answer.contains("entries:[]"), "{answer}");
    assert!(answer.contains("Nothing.vst3"), "{answer}");
    assert!(answer.contains("is not a VST3 bundle"), "{answer}");
}

#[test]
fn the_flag_without_a_bundle_is_refused_with_exit_2_and_what_to_write() {
    let output = vix_mix().arg("--scan-bundle").output().unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--scan-bundle ~/.vst3/Echo.vst3"),
        "{stderr}"
    );
    assert!(output.stdout.is_empty(), "{output:?}");
}
