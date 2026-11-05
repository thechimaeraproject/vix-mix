//! The winit application: the window, the host, and one frame at a time.
//!
//! **Pacing.** The event loop is `ControlFlow::Wait`. A frame is drawn when egui wants one
//! for input, when egui's own `repaint_delay` (a tooltip fade) comes due, and — while the
//! meters move (the host's backend is running or simulated) or a `--quit-after-frames` count
//! is still running — at most every [`FRAME_INTERVAL`], so the window never repaints faster
//! than 60 Hz. Between frames the host is polled on its own when it asks
//! ([`Host::next_poll`]: the backend's news, the debounced save, an undo running out), without
//! a frame unless the poll brought news. With nothing to animate and nothing pending in the
//! host the process is idle.
//!
//! **The banner line** shows the backend's banner, else the newest toast for [`NOTE_FOR`],
//! else the first insert playing as a pass-through: one line for everything the user must read
//! until the window's own toasts and slot marks (handoff 05) take them.

use std::sync::Arc;
use std::time::{Duration, Instant};

use vixmix_host::{AudioBackend, BackendState, BootOptions, FakeHost, Host, HostEvent, RealHost};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::Startup;
use crate::gpu::Gpu;
use crate::meters::Meters;
use crate::ui;

/// The shortest time between two frames: 60 Hz.
pub const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// How long a toast stays in the banner line.
pub const NOTE_FOR: Duration = Duration::from_secs(6);

/// The winit application.
pub struct App {
    startup: Startup,
    running: Option<Running>,
    failure: Option<String>,
}

impl App {
    pub fn new(startup: Startup) -> App {
        App {
            startup,
            running: None,
            failure: None,
        }
    }

    /// Why the window could not run, if it could not.
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }
}

/// Everything that exists while the window does.
struct Running {
    window: Arc<Window>,
    gpu: Gpu,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    host: Box<dyn Host>,
    backend: BackendState,
    /// The meters' levels and peak-hold lines, carried from frame to frame.
    meters: Meters,
    /// When the window opened: the peak-hold clock.
    opened: Instant,
    first_frame: bool,
    frames: u64,
    quit_after_frames: Option<u64>,
    last_frame: Instant,
    /// The newest toast and when it came, for the banner line.
    note: Option<(String, Instant)>,
    /// When the next frame is due, and when the host wants its next poll.
    frame_at: Option<Instant>,
    poll_at: Option<Instant>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        match Running::new(event_loop, &self.startup) {
            Ok(running) => {
                running.window.request_redraw();
                self.running = Some(running);
            }
            Err(problem) => {
                self.failure = Some(problem);
                event_loop.exit();
            }
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        let (StartCause::ResumeTimeReached { .. }, Some(running)) = (cause, &mut self.running)
        else {
            return;
        };
        let now = Instant::now();
        // The frame timer (meters, a quit count) or egui's `repaint_delay` has come due.
        if running.frame_at.is_some_and(|at| at <= now) {
            running.window.request_redraw();
            return;
        }
        // Only the host's poll is due: a frame only if it brought news.
        if running.poll_at.is_some_and(|at| at <= now) {
            if running.poll_host() {
                running.window.request_redraw();
                return;
            }
            running.poll_at = running.host.next_poll().map(|wait| now + wait);
            running.schedule(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(running) = &mut self.running else {
            return;
        };
        if id != running.window.id() {
            return;
        }
        // The redraw itself is not input: egui-winit answers it with `repaint: true`, and
        // honouring that would make every frame request the next one.
        if event == WindowEvent::RedrawRequested {
            if let Err(problem) = running.frame(event_loop) {
                self.failure = Some(problem);
                event_loop.exit();
            }
            return;
        }
        let response = running.egui_state.on_window_event(&running.window, &event);
        if response.repaint {
            running.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => running.gpu.resize(size.width, size.height),
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // The host goes first (its editors close before their windows), then the window.
        if let Some(running) = self.running.take() {
            let Running { host, window, .. } = running;
            drop(host);
            drop(window);
        }
    }
}

impl Running {
    fn new(event_loop: &ActiveEventLoop, startup: &Startup) -> Result<Running, String> {
        let host: Box<dyn Host> = if startup.fake {
            Box::new(FakeHost::new())
        } else {
            Box::new(
                RealHost::boot(BootOptions {
                    config_dir: None,
                    debug_build: cfg!(debug_assertions),
                    audio: AudioBackend::APP,
                    plugin_dirs: None,
                })
                .map_err(|e| e.to_string())?,
            )
        };
        let attributes = Window::default_attributes()
            .with_title("Vix Mix")
            .with_theme(Some(winit::window::Theme::Dark))
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(480.0, 320.0));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| format!("no window: {e}"))?,
        );
        let gpu = Gpu::new(Arc::clone(&window), ui::clear_colour())?;

        let egui_ctx = egui::Context::default();
        ui::install(&egui_ctx);
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(gpu.max_texture_side()),
        );
        Ok(Running {
            window,
            gpu,
            egui_ctx,
            egui_state,
            host,
            backend: BackendState::Connecting,
            meters: Meters::default(),
            opened: Instant::now(),
            first_frame: true,
            frames: 0,
            quit_after_frames: startup.quit_after_frames,
            last_frame: Instant::now(),
            note: None,
            frame_at: None,
            poll_at: None,
        })
    }

    /// Polls the host and takes in what it reports; whether it reported anything.
    fn poll_host(&mut self) -> bool {
        let events = self.host.poll();
        let news = !events.is_empty();
        for event in events {
            match event {
                HostEvent::Backend(state) => self.backend = state,
                HostEvent::Toast(message) => {
                    log::warn!("{message}");
                    self.note = Some((message, Instant::now()));
                }
                HostEvent::Removed { message, .. } => {
                    log::info!("{message}");
                    self.note = Some((message, Instant::now()));
                }
                _ => {}
            }
        }
        news
    }

    /// What the banner line shows: the backend's banner, else the newest toast, else the
    /// first insert playing as a pass-through. The window's own toasts and slot marks
    /// (handoff 05) replace the last two.
    fn banner(&self) -> BackendState {
        if self.backend.banner().is_some() {
            return self.backend.clone();
        }
        let toast = self
            .note
            .as_ref()
            .filter(|(_, at)| at.elapsed() < NOTE_FOR)
            .map(|(message, _)| message.clone());
        let problem = || self.host.problems().first().map(|p| p.message.clone());
        match toast.or_else(problem) {
            Some(reason) => BackendState::Absent { reason },
            None => self.backend.clone(),
        }
    }

    /// Sleeps until the next frame or the host's next poll, whichever comes first.
    fn schedule(&self, event_loop: &ActiveEventLoop) {
        match [self.frame_at, self.poll_at].into_iter().flatten().min() {
            Some(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        self.poll_host();
        let now_ms = self.opened.elapsed().as_secs_f64() * 1000.0;
        self.meters.update(&self.host.meters(), now_ms);
        let shown = self.banner();
        let view = ui::View {
            backend: &shown,
            stats: self.host.stats(),
            setup: self.host.setup(),
            meters: &self.meters,
            apps: self.host.apps(),
        };

        let mut raw_input = self.egui_state.take_egui_input(&self.window);
        let info = raw_input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default();
        egui_winit::update_viewport_info(info, &self.egui_ctx, &self.window, self.first_frame);
        self.first_frame = false;
        let output = self
            .egui_ctx
            .run_ui(raw_input, |root| ui::draw(root, &view));
        let egui::FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point,
            viewport_output,
        } = output;
        self.egui_state
            .handle_platform_output(&self.window, platform_output);
        let drawn = self
            .gpu
            .render(&self.egui_ctx, shapes, textures_delta, pixels_per_point)?;
        if drawn {
            self.frames += 1;
            self.last_frame = Instant::now();
        }
        if self.quit_after_frames.is_some_and(|n| self.frames >= n) {
            log::info!(
                "drew {} frames; closing as --quit-after-frames asked",
                self.frames
            );
            event_loop.exit();
            return Ok(());
        }

        // Pacing: the next frame now, at a time, or not until something happens.
        let egui_delay = viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(Duration::MAX, |v| v.repaint_delay);
        // A toast in the banner line is redrawn when it runs out.
        let note_delay = self
            .note
            .as_ref()
            .and_then(|(_, at)| NOTE_FOR.checked_sub(at.elapsed()))
            .unwrap_or(Duration::MAX);
        let animating = self.backend.live() || self.quit_after_frames.is_some();
        let delay = if animating {
            egui_delay.min(FRAME_INTERVAL)
        } else {
            egui_delay.min(note_delay)
        };
        let now = Instant::now();
        // Never sooner than one frame interval after the last frame: at most 60 Hz.
        self.frame_at =
            (delay != Duration::MAX).then(|| (now + delay).max(self.last_frame + FRAME_INTERVAL));
        self.poll_at = self.host.next_poll().map(|wait| now + wait);
        self.schedule(event_loop);
        Ok(())
    }
}
