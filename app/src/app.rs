//! The winit application: the window, the host, the plugin editor windows, and one frame at
//! a time.
//!
//! **A frame.** The host's events (the backend's state, a toast, an editor it closed), its
//! meters into the window's [`Meters`](crate::meters::Meters), the open editors served;
//! then egui runs [`ui::draw`], which returns what the gestures asked; the [`Shell`] carries
//! that out against the host; editors it queued open in windows of their own; the window
//! keeps at least the console's size.
//!
//! **Pacing** is the [`Pacer`]'s ([`crate::pacing`]): the event loop sleeps until the frame
//! or the hidden tick it names. Frames come only while the window can be seen: on a 60 Hz
//! grid while the meters move, when egui or a gesture wants one, at least every
//! [`EDITOR_POLL`](crate::editors::EDITOR_POLL) while an editor is open; never more than 60 a
//! second. Minimised, zero-sized or fully covered, it draws nothing and serves the host and
//! the editors without a frame. With no backend, no editor and nothing to animate it is idle.
//!
//! **Viewport facts.** egui's facts about the window (where it is, its monitor, whether it is
//! minimised) are asked of the desktop when a window event may have changed them and at most
//! every [`VIEWPORT_CHECK`] otherwise, not every frame: on X11 each is a round trip.

use std::sync::Arc;
use std::time::Instant;

use vixmix_host::{AudioBackend, BootOptions, FakeHost, Host, RealHost};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::Startup;
use crate::editors::EditorWindows;
use crate::gpu::Gpu;
use crate::pacing::{Next, Pacer, VIEWPORT_CHECK, Wake, Wants};
use crate::shell::Shell;
use crate::ui;

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
    /// When the next frame or hidden tick comes, and whether the window can be seen.
    pacer: Pacer,
    /// When egui's viewport facts were last asked of the desktop; `None` when an event may
    /// have changed them.
    viewport_checked: Option<Instant>,
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

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        // The pacer's timer: a frame (meters, a quit count, editors, egui's `repaint_delay`),
        // or, while the window is hidden, the host and the editors served without one.
        let (StartCause::ResumeTimeReached { .. }, Some(running)) = (cause, &mut self.running)
        else {
            return;
        };
        match running.pacer.next() {
            Next::Draw(_) => running.window.request_redraw(),
            Next::Serve(_) => running.serve(event_loop),
            Next::Wait => {}
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
            if let Err(problem) = running.redraw(event_loop) {
                self.failure = Some(problem);
                event_loop.exit();
            }
            return;
        }
        let response = running.egui_state.on_window_event(&running.window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                running.gpu.resize(size.width, size.height);
                running.seen(|seen| seen.zero_size = size.width == 0 || size.height == 0);
            }
            WindowEvent::Occluded(occluded) => running.seen(|seen| seen.occluded = occluded),
            WindowEvent::Moved(_)
            | WindowEvent::Focused(_)
            | WindowEvent::ScaleFactorChanged { .. } => running.seen(|_| {}),
            _ => {}
        }
        if response.repaint {
            running.wake(event_loop);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // Every editor closes through the host before its window; then the host goes (its
        // instances after their editors), then the window.
        if let Some(mut running) = self.running.take() {
            running.close_editors();
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
            pacer: Pacer::new(Instant::now()),
            viewport_checked: None,
            first_drawn: None,
            min_size: natural,
        })
    }

    /// Closes every open editor through the host. At exit it comes before the host goes.
    fn close_editors(&mut self) {
        self.editors.close_all(self.host.as_mut());
    }

    /// Whether the meters move or a `--quit-after-frames` count runs: frames on the grid.
    fn animating(&self) -> bool {
        self.shell.backend.live() || self.quit_after_frames.is_some()
    }

    /// A change to whether the window can be seen, from one of its events: the desktop is
    /// asked again whether it is minimised, egui's viewport facts are asked again at the next
    /// frame, and a window that has just come back into view draws.
    fn seen(&mut self, change: impl FnOnce(&mut crate::pacing::Visibility)) {
        let was_hidden = self.pacer.seen.hidden();
        change(&mut self.pacer.seen);
        self.pacer.seen.minimized = self.window.is_minimized() == Some(true);
        self.viewport_checked = None;
        if was_hidden && !self.pacer.seen.hidden() {
            self.window.request_redraw();
        }
    }

    /// Asks the desktop again for egui's viewport facts when an event may have changed them
    /// or [`VIEWPORT_CHECK`] has passed; whether it is minimised comes with them.
    fn refresh_viewport(&mut self, now: Instant) {
        if self
            .viewport_checked
            .is_some_and(|at| now.saturating_duration_since(at) < VIEWPORT_CHECK)
        {
            return;
        }
        let info = self
            .egui_state
            .egui_input_mut()
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default();
        egui_winit::update_viewport_info(info, &self.egui_ctx, &self.window, self.first_frame);
        self.pacer.seen.minimized = info.minimized == Some(true);
        self.viewport_checked = Some(now);
    }

    /// A redraw the window was asked for: a frame, or, while it is hidden, a tick without one.
    fn redraw(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.pacer.seen.hidden() {
            // Asked to draw while hidden (an X11 expose as it comes back): the desktop's word.
            self.pacer.seen.minimized = self.window.is_minimized() == Some(true);
            if self.pacer.seen.hidden() {
                self.serve(event_loop);
                return Ok(());
            }
        }
        self.frame(event_loop)
    }

    /// The host's events taken in and the open editors served.
    fn serve_host(&mut self) {
        for event in self.host.poll() {
            self.shell.event(event, &mut self.editors);
        }
        self.editors.tick(self.host.as_mut());
    }

    /// The hidden window's tick: the host and the editors served, nothing drawn. A window back
    /// in view with no event that said so (asked of the desktop each tick) draws instead.
    fn serve(&mut self, event_loop: &ActiveEventLoop) {
        let slot = self.pacer.begin(Instant::now());
        self.pacer.seen.minimized = self.window.is_minimized() == Some(true);
        if !self.pacer.seen.hidden() {
            self.window.request_redraw();
            return;
        }
        self.serve_hidden(event_loop, slot);
    }

    fn serve_hidden(&mut self, event_loop: &ActiveEventLoop, slot: Instant) {
        self.serve_host();
        if !self.editors.is_empty() {
            self.editors.follow(self.host.as_mut());
        }
        let wants = Wants {
            repaint: None,
            animating: self.animating(),
            editors: !self.editors.is_empty(),
            acted: false,
        };
        let next = self.pacer.end(slot, Instant::now(), None, wants);
        schedule(event_loop, next);
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let started = Instant::now();
        let slot = self.pacer.begin(started);
        self.refresh_viewport(started);
        if self.pacer.seen.hidden() {
            // The desktop says it is minimised: nothing to draw.
            self.serve_hidden(event_loop, slot);
            return Ok(());
        }
        self.serve_host();
        let now_ms = self.opened.elapsed().as_secs_f64() * 1000.0;
        self.shell.meters.update(&self.host.meters(), now_ms);

        let raw_input = self.egui_state.take_egui_input(&self.window);
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
        self.shell
            .open_editors(event_loop, self.host.as_mut(), &mut self.editors);
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
        // Only a frame that reached the screen counts; a refused one still takes its slot.
        if drawn {
            self.frames += 1;
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

        let wants = Wants {
            repaint: viewport_output
                .get(&egui::ViewportId::ROOT)
                .map(|v| v.repaint_delay),
            animating: self.animating(),
            editors: !self.editors.is_empty(),
            acted,
        };
        let next = self.pacer.end(slot, Instant::now(), Some(drawn), wants);
        schedule(event_loop, next);
        Ok(())
    }

    /// Asks for a frame for input as soon as the 60 Hz cap allows, and none while hidden.
    fn wake(&mut self, event_loop: &ActiveEventLoop) {
        match self.pacer.wake(Instant::now()) {
            Wake::Now => self.window.request_redraw(),
            Wake::At(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            Wake::Nothing => {}
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

/// Sleeps the event loop until `next`, or until an event.
fn schedule(event_loop: &ActiveEventLoop, next: Next) {
    event_loop.set_control_flow(next.at().map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
}
