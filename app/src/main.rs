//! The Vix Mix window: winit, egui through egui-winit and egui-wgpu, and one [`Host`] it
//! drives. Not eframe, because each VST3 editor later needs a winit window of its own with a
//! raw window handle, which Vix's shell already does.
//!
//! Modules: [`app`] is the winit application and the frame loop, paced by [`pacing`]; [`gpu`]
//! the surface and the egui renderer; [`ui`] what each frame draws and senses: the header and
//! the [`console`] of [`strip`]s (each with its [`fader`]s and its FX [`well`], inserts dragged
//! by [`drag`]), the [`popover`]s and the plugin [`browser`], the [`toast`] line, in the
//! [`theme`]'s colours, sizes and fonts, with the prototype's [`icons`], the shared
//! [`widgets`] and the [`meters`]' levels and peak-hold lines; [`shell`] the window's state
//! between frames and what it does with a frame's gestures; [`editors`] the plugin editor
//! windows; [`capture`] the look gate's offscreen frame.
//!
//! The command line is for checks, not for users: `--fake` runs against `FakeHost` (prototype
//! v5's example data and simulated meters) instead of the real host,
//! `--quit-after-frames N` closes the window once N frames have been drawn, so a headless run
//! proves the window works, and `--dump-png <file>` draws `FakeHost`'s console once, offscreen
//! at the prototype's scale, into a PNG. `--scan-bundle <bundle>` is for the plugin scan, which
//! runs this program with it for each bundle it must load, so a plugin that crashes while it
//! loads takes only that process down. See `docs/reference/app.md`.
//!
//! [`Host`]: vixmix_host::Host

#[cfg(test)]
mod acceptance;
mod app;
mod browser;
mod capture;
mod console;
mod drag;
mod editors;
mod fader;
mod gpu;
mod icons;
mod meters;
mod pacing;
mod popover;
mod shell;
mod strip;
#[cfg(test)]
mod test_support;
mod theme;
mod toast;
mod ui;
mod well;
mod widgets;

use std::path::PathBuf;

use vixmix_host::{Describe, ScanCommand};
use winit::error::EventLoopError;
use winit::event_loop::EventLoop;

const USAGE: &str = "\
usage: vix-mix [--fake] [--quit-after-frames <n>] | --dump-png <file> | --scan-bundle <bundle>
  --fake               run against FakeHost: the prototype's example console, simulated meters, no audio
  --quit-after-frames  close the window once this many frames have been drawn
  --dump-png           draw FakeHost's console once, offscreen at twice the size, into a PNG, and exit
  --scan-bundle        describe one VST3 bundle on a marked line of stdout and exit: the plugin scan runs each bundle it loads this way";

/// The flag of the plugin scan's child process: Vix Mix runs itself with it for each bundle
/// its scan must load ([`plugin_scan`]).
const SCAN_BUNDLE: &str = "--scan-bundle";

/// What the command line asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Startup {
    pub fake: bool,
    pub quit_after_frames: Option<u64>,
    /// Draw one frame into this PNG instead of opening the window.
    pub dump_png: Option<PathBuf>,
    /// Describe this bundle for a plugin scan in another process, and exit.
    pub scan_bundle: Option<PathBuf>,
}

impl Startup {
    fn parse(args: &[String]) -> Result<Startup, String> {
        let mut startup = Startup::default();
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            match arg.as_str() {
                "--fake" => startup.fake = true,
                "--quit-after-frames" => {
                    let value = rest.next().ok_or_else(|| {
                        "`--quit-after-frames` wants a number of frames, e.g. `--quit-after-frames 3`"
                            .to_string()
                    })?;
                    let frames: u64 = value.parse().map_err(|_| {
                        format!(
                            "`--quit-after-frames {value}` is not a number of frames; write a whole number, e.g. 3"
                        )
                    })?;
                    if frames == 0 {
                        return Err(
                            "`--quit-after-frames 0` would close before drawing anything; ask for 1 or more"
                                .to_string(),
                        );
                    }
                    startup.quit_after_frames = Some(frames);
                }
                "--dump-png" => {
                    let path = rest.next().ok_or_else(|| {
                        "`--dump-png` wants the file to write, e.g. `--dump-png look-gate.png`"
                            .to_string()
                    })?;
                    startup.dump_png = Some(PathBuf::from(path));
                }
                SCAN_BUNDLE => {
                    let path = rest.next().ok_or_else(|| {
                        "`--scan-bundle` wants the VST3 bundle to describe, e.g. `--scan-bundle ~/.vst3/Echo.vst3`"
                            .to_string()
                    })?;
                    startup.scan_bundle = Some(PathBuf::from(path));
                }
                other => {
                    return Err(format!(
                        "`{other}` is not something vix-mix takes; it takes --fake, --quit-after-frames <n>, --dump-png <file> and --scan-bundle <bundle>"
                    ));
                }
            }
        }
        if startup.dump_png.is_some() && startup.quit_after_frames.is_some() {
            return Err(
                "`--dump-png` draws one frame without a window, so `--quit-after-frames` has nothing to close; drop one of them"
                    .to_string(),
            );
        }
        let with_scan_bundle = Startup {
            scan_bundle: startup.scan_bundle.clone(),
            ..Startup::default()
        };
        if startup.scan_bundle.is_some() && startup != with_scan_bundle {
            return Err(
                "`--scan-bundle` describes one plugin bundle and exits without a window or a host, so it takes nothing else; drop the other flags"
                    .to_string(),
            );
        }
        Ok(startup)
    }
}

/// How the real host describes a plugin bundle it must load: in a child process of this
/// program (`--scan-bundle`), one bundle at a time, so a plugin that crashes or hangs while it
/// loads fails alone and Vix Mix keeps running. In this process, with a warning, when this
/// program cannot name itself.
fn plugin_scan() -> Describe {
    match std::env::current_exe() {
        Ok(program) => Describe::InChild(ScanCommand::new(program, [SCAN_BUNDLE])),
        Err(error) => {
            log::warn!(
                "the plugin scan loads plugins in this process, so one that crashes while it loads closes Vix Mix: this program's own path could not be read ({error})"
            );
            Describe::InProcess
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let startup = match Startup::parse(&args) {
        Ok(startup) => startup,
        Err(problem) => {
            eprintln!("vix-mix: {problem}\n{USAGE}");
            std::process::exit(2);
        }
    };
    if let Some(bundle) = &startup.scan_bundle {
        std::process::exit(vixmix_host::answer_scan(bundle));
    }
    if let Some(path) = &startup.dump_png {
        match capture::dump_png(path) {
            Ok([width, height]) => {
                log::info!("wrote {} ({width} x {height} pixels)", path.display());
                return;
            }
            Err(problem) => {
                eprintln!("vix-mix: {problem}");
                std::process::exit(1);
            }
        }
    }
    let event_loop = match event_loop() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            eprintln!("vix-mix: no event loop: {e}. Expected a desktop session with a display");
            std::process::exit(1);
        }
    };
    // Idle means idle: the loop sleeps until the window, a meter or egui's timer wakes it.
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    let mut app = app::App::new(startup);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("vix-mix: the event loop failed: {e}");
        std::process::exit(1);
    }
    if let Some(problem) = app.failure() {
        eprintln!("vix-mix: {problem}");
        std::process::exit(1);
    }
}

/// The event loop. On Linux it runs on X11 (XWayland under a Wayland desktop) whenever there
/// is an X display, because a VST3 editor embeds only in an X11 window and the kit refuses
/// Wayland handles; with no X display it runs where it can and says that editors cannot
/// open. (winit makes one event loop a process, so the choice is made before trying.)
fn event_loop() -> Result<EventLoop<()>, EventLoopError> {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11;
        if std::env::var_os("DISPLAY").is_some() {
            return EventLoop::builder().with_x11().build();
        }
        log::warn!(
            "no X display (DISPLAY is unset): the window runs on Wayland, where plugin editors cannot open; run a session with XWayland for them"
        );
    }
    EventLoop::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn the_flags_parse_and_default_to_the_real_host_running_until_closed() {
        assert_eq!(Startup::parse(&[]).unwrap(), Startup::default());
        let parsed = Startup::parse(&args(&["--quit-after-frames", "3", "--fake"])).unwrap();
        assert_eq!(
            parsed,
            Startup {
                fake: true,
                quit_after_frames: Some(3),
                dump_png: None,
                scan_bundle: None,
            }
        );
        let dump = Startup::parse(&args(&["--dump-png", "gate.png"])).unwrap();
        assert_eq!(dump.dump_png, Some(PathBuf::from("gate.png")));
        let scan = Startup::parse(&args(&["--scan-bundle", "/usr/lib64/vst3/Echo.vst3"])).unwrap();
        assert_eq!(
            scan,
            Startup {
                scan_bundle: Some(PathBuf::from("/usr/lib64/vst3/Echo.vst3")),
                ..Startup::default()
            }
        );
    }

    #[test]
    fn the_real_host_scans_each_bundle_it_loads_in_this_program_with_the_flag() {
        let Describe::InChild(command) = plugin_scan() else {
            panic!("this program can name itself");
        };
        assert_eq!(command.program, std::env::current_exe().unwrap());
        assert_eq!(command.args, [std::ffi::OsString::from(SCAN_BUNDLE)]);
        assert_eq!(command.timeout, vixmix_plugins::SCAN_TIMEOUT);
        let parsed = Startup::parse(&args(&[SCAN_BUNDLE, "Echo.vst3"])).unwrap();
        assert_eq!(parsed.scan_bundle, Some(PathBuf::from("Echo.vst3")));
    }

    #[test]
    fn every_refusal_names_what_to_write_instead() {
        for (words, expected) in [
            (vec!["--quit-after-frames"], "--quit-after-frames 3"),
            (vec!["--quit-after-frames", "soon"], "whole number"),
            (vec!["--quit-after-frames", "0"], "1 or more"),
            (
                vec!["--fullscreen"],
                "--fake, --quit-after-frames <n>, --dump-png <file> and --scan-bundle <bundle>",
            ),
            (vec!["--dump-png"], "--dump-png look-gate.png"),
            (
                vec!["--dump-png", "a.png", "--quit-after-frames", "3"],
                "drop one of them",
            ),
            (vec!["--scan-bundle"], "--scan-bundle ~/.vst3/Echo.vst3"),
            (
                vec!["--fake", "--scan-bundle", "Echo.vst3"],
                "drop the other flags",
            ),
        ] {
            let error = Startup::parse(&args(&words)).expect_err("refused");
            assert!(error.contains(expected), "{words:?} said `{error}`");
        }
    }
}
