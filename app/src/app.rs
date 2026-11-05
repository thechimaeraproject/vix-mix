//! The winit application: the window, the host, the plugin editor windows, and one frame at
//! a time.
//!
//! **A frame.** The host's events (the backend's state, a toast, an editor it closed), its
//! meters into the window's [`Meters`](crate::meters::Meters), the open editors served;
//! then egui runs [`ui::draw`], which returns what the gestures asked; the [`Shell`] carries
//! that out against the host; editors it queued open in windows of their own; the window
//! keeps at least the console's size.
//!
//! **Pacing.** The event loop is `ControlFlow::Wait`. A frame is drawn when egui wants one
//! (input, or its `repaint_delay` coming due: a tooltip, the toast's fade, a text cursor),
//! straight after a frame that acted (what a command changed shows at once), and — while the
//! meters move (the host's backend is running or simulated) or a `--quit-after-frames` count
//! is still running — on a 60 Hz grid: a frame the timer wakes counts from when it was due,
//! so neither drawing it nor the timer's lateness stretches the interval to the next. No
//! frame, input's included, is due sooner than [`FRAME_INTERVAL`] after the last was due, so
//! the window never draws more than 60 frames a second; while a plugin editor is open it
//! draws at least every [`EDITOR_POLL`]. With no backend, no editor and nothing to animate
//! it is idle.

use std::sync::Arc;
use std::time::{Duration, Instant};

use vixmix_host::{AudioBackend, BootOptions, FakeHost, Host, RealHost};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::Startup;
use crate::editors::{EDITOR_POLL, EditorWindows};
use crate::gpu::Gpu;
use crate::shell::Shell;
use crate::theme::pop;
use crate::ui;

/// The shortest time between two frames: 60 Hz.
pub const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

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
    /// The window's state between frames: meters, gestures, the popover, the toasts.
    shell: Shell,
    /// The plugin editors' windows.
    editors: EditorWindows,
    /// When the window opened: the peak-hold clock.
    opened: Instant,
    first_frame: bool,
    frames: u64,
    quit_after_frames: Option<u64>,
    /// When the last frame that reached the screen began (or was due, when the timer woke
    /// it): the next is never sooner than one interval after.
    last_frame: Instant,
    /// When the timer is set to wake the next frame.
    due: Option<Instant>,
    /// When the first frame reached the screen.
    first_drawn: Option<Instant>,
    /// The least size the window has been given, in points: what the content needs.
    min_size: egui::Vec2,
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

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        // The frame timer (meters, a quit count, editors) or egui's `repaint_delay` is due.
        if let (StartCause::ResumeTimeReached { .. }, Some(running)) = (cause, &self.running) {
            running.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(running) = &mut self.running else {
            return;
        };
        if id != running.window.id() {
            // A plugin editor's window: closing it closes its editor through the host first.
            running
                .editors
                .window_event(running.host.as_mut(), id, &event);
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
            running.wake(event_loop);
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => running.gpu.resize(size.width, size.height),
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // Every editor closes through the host before its window; then the host goes (its
        // instances after their editors), then the window.
        if let Some(running) = self.running.take() {
            let Running {
                mut host,
                mut editors,
                window,
                ..
            } = running;
            editors.close_all(host.as_mut());
            drop(editors);
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
        // The console's own size, as the prototype's window is; the first frame grows it to
        // fit a banner.
        let natural = ui::natural_size(host.setup());
        let attributes = Window::default_attributes()
            .with_title("Vix Mix")
            .with_theme(Some(winit::window::Theme::Dark))
            .with_inner_size(winit::dpi::LogicalSize::new(natural.x, natural.y))
            .with_min_inner_size(winit::dpi::LogicalSize::new(natural.x, natural.y));
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
            shell: Shell::default(),
            editors: EditorWindows::default(),
            opened: Instant::now(),
            first_frame: true,
            frames: 0,
            quit_after_frames: startup.quit_after_frames,
            last_frame: Instant::now(),
            due: None,
            first_drawn: None,
            min_size: natural,
        })
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        // Frames are spaced from start to start, so a frame's own drawing and presenting do
        // not stretch the interval to the next; a frame woken by the timer counts from when
        // it was due, so the timer's lateness does not either.
        let started = Instant::now();
        let slot = match self.due.take() {
            Some(due) if started >= due && started - due < FRAME_INTERVAL => due,
            _ => started,
        };
        for event in self.host.poll() {
            self.shell.event(event, &mut self.editors);
        }
        let now_ms = self.opened.elapsed().as_secs_f64() * 1000.0;
        self.shell.meters.update(&self.host.meters(), now_ms);
        self.editors.tick(self.host.as_mut());

        let mut raw_input = self.egui_state.take_egui_input(&self.window);
        let info = raw_input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default();
        egui_winit::update_viewport_info(info, &self.egui_ctx, &self.window, self.first_frame);
        self.first_frame = false;
        let mut intents = Vec::new();
        let (shell, host) = (&mut self.shell, &*self.host);
        let output = self.egui_ctx.run_ui(raw_input, |root| {
            intents = ui::draw(root, shell, host);
        });
        // A frame that acted draws again at once: what it changed shows on the next.
        let acted = !intents.is_empty();
        self.shell
            .dispatch(self.host.as_mut(), &mut self.editors, intents);
        for instance in std::mem::take(&mut self.shell.to_open) {
            if let Err(problem) = self.editors.open(event_loop, self.host.as_mut(), instance) {
                log::warn!("{problem}");
                self.shell
                    .toasts
                    .show(problem, None, self.shell.now, pop::TOAST_HOST_S);
            }
        }
        if !self.editors.is_empty() {
            self.editors.follow(self.host.as_mut());
        }
        self.fit();

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
            self.last_frame = slot;
            self.first_drawn.get_or_insert(started);
        }
        if self.quit_after_frames.is_some_and(|n| self.frames >= n) {
            let took = self.first_drawn.map_or(0.0, |t| t.elapsed().as_secs_f64());
            log::info!(
                "drew {} frames in {took:.2} s after the first ({:.1} a second); closing as --quit-after-frames asked",
                self.frames,
                (self.frames - 1) as f64 / took.max(f64::EPSILON),
            );
            event_loop.exit();
            return Ok(());
        }

        // Pacing: the next frame at a time, or not until something happens. egui's own
        // delay counts from now; the meters' and the editors' cadence from when the last
        // frame began, so drawing a frame does not stretch the interval to the next.
        let now = Instant::now();
        let mut due = viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.repaint_delay)
            .filter(|d| *d != Duration::MAX)
            .map(|d| now + d);
        let mut sooner = |at: Instant| due = Some(due.map_or(at, |d| d.min(at)));
        if self.shell.backend.live() || self.quit_after_frames.is_some() {
            sooner(self.last_frame + FRAME_INTERVAL);
        }
        if !self.editors.is_empty() {
            sooner(self.last_frame + EDITOR_POLL);
        }
        if acted {
            sooner(now);
        }
        match due {
            None => event_loop.set_control_flow(ControlFlow::Wait),
            // Never sooner than one frame interval after the last frame began: at most 60 Hz.
            Some(at) => {
                let at = at.max(self.last_frame + FRAME_INTERVAL);
                self.due = Some(at);
                event_loop.set_control_flow(ControlFlow::WaitUntil(at));
            }
        }
        Ok(())
    }

    /// Asks for a frame as soon as the 60 Hz cap allows: at once, or one interval after the
    /// last frame began. Input never draws faster than the meters do.
    fn wake(&mut self, event_loop: &ActiveEventLoop) {
        let at = self.last_frame + FRAME_INTERVAL;
        if Instant::now() >= at {
            self.window.request_redraw();
        } else if self.due.is_none_or(|due| at < due) {
            self.due = Some(at);
            event_loop.set_control_flow(ControlFlow::WaitUntil(at));
        }
    }

    /// Keeps the window at least as big as the content the last frame drew (the console,
    /// and the banner above it when there is one): its least size follows, and a window
    /// smaller than that grows to it.
    fn fit(&mut self) {
        let needed = self.shell.content.max.to_vec2();
        if needed.x <= 0.0 || needed == self.min_size {
            return;
        }
        self.min_size = needed;
        let size = winit::dpi::LogicalSize::new(needed.x, needed.y);
        self.window.set_min_inner_size(Some(size));
        let scale = self.window.scale_factor();
        let now: winit::dpi::LogicalSize<f32> = self.window.inner_size().to_logical(scale);
        if now.width < needed.x || now.height < needed.y {
            let _ = self.window.request_inner_size(winit::dpi::LogicalSize::new(
                now.width.max(needed.x),
                now.height.max(needed.y),
            ));
        }
    }
}
