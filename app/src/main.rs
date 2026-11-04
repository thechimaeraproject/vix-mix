//! The Vix Mix window: winit, egui through egui-winit and egui-wgpu, and one [`Host`] it
//! drives. Not eframe, because each VST3 editor later needs a winit window of its own with a
//! raw window handle, which Vix's shell already does.
//!
//! Modules: [`app`] is the winit application and the frame loop; [`gpu`] the surface and the
//! egui renderer; [`ui`] what each frame draws.
//!
//! The command line is for checks, not for users: `--fake` runs against `FakeHost` (prototype
//! v5's example data and simulated meters) instead of the real host, and
//! `--quit-after-frames N` closes the window once N frames have been drawn, so a headless run
//! proves the window works. See `docs/reference/app.md`.
//!
//! [`Host`]: vixmix_host::Host

mod app;
mod gpu;
mod ui;

use winit::event_loop::EventLoop;

const USAGE: &str = "\
usage: vix-mix [--fake] [--quit-after-frames <n>]
  --fake               run against FakeHost: the prototype's example console, simulated meters, no audio
  --quit-after-frames  close the window once this many frames have been drawn";

/// What the command line asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Startup {
    pub fake: bool,
    pub quit_after_frames: Option<u64>,
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
                other => return Err(format!("`{other}` is not something vix-mix takes")),
            }
        }
        Ok(startup)
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
    let event_loop = match EventLoop::new() {
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
                quit_after_frames: Some(3)
            }
        );
    }

    #[test]
    fn every_refusal_names_what_to_write_instead() {
        for (words, expected) in [
            (vec!["--quit-after-frames"], "--quit-after-frames 3"),
            (vec!["--quit-after-frames", "soon"], "whole number"),
            (vec!["--quit-after-frames", "0"], "1 or more"),
            (vec!["--fullscreen"], "--fullscreen"),
        ] {
            let error = Startup::parse(&args(&words)).expect_err("refused");
            assert!(error.contains(expected), "{words:?} said `{error}`");
        }
    }
}
