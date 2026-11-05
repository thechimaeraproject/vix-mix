//! Plugin editor windows: one winit window per open editor, which the plugin draws into
//! itself (egui never does), titled "<plugin> · <strip> · slot N". Vix's `open_editor` and
//! `EditorWindow` are the model.
//!
//! - **Open**: a window is made hidden and fixed-size, its raw handle goes to
//!   `Host::open_editor`, and the window takes the size the plugin asks for and shows. A
//!   refusal (no editor, `FakeHost`, a Wayland handle on Linux) drops the window and says why.
//!   Opening an editor that is open raises its window.
//! - **Resize**: `Host::tick_editors` names the instance that asked; its window is resized.
//! - **Close**: always through the host before the window goes — the window's close button,
//!   the insert's removal (the shell closes it before the command), the window closing — and
//!   an editor the host closed itself (`HostEvent::EditorClosed`) loses its window.
//! - **Linux**: the event loop runs on X11 (XWayland under KDE Wayland), because the kit
//!   refuses Wayland handles (`main.rs`). The host polls each editor's run loop in
//!   `tick_editors`, which the frame calls at least every [`EDITOR_POLL`].

use std::time::Duration;

use chimaeraproject_vst3_host::EditorEvent;
use vixmix_host::Host;
use vixmix_model::{InstanceId, MixId, Setup, Strip};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{Window, WindowId};

/// How often the frame comes round while an editor is open: a plugin's window is served
/// (its Linux run loop, its resize requests) only when the host is asked.
pub const EDITOR_POLL: Duration = Duration::from_millis(33);

/// The most editor windows open at once; a further one is refused with a toast.
pub const MAX_EDITORS: usize = 16;

/// The size an editor's window is made at, hidden, before its plugin says what it wants.
const FIRST_SIZE: [u32; 2] = [640, 480];

/// One open editor.
struct EditorWindow {
    instance: InstanceId,
    window: Window,
    title: String,
}

/// Every open editor's window.
#[derive(Default)]
pub struct EditorWindows {
    windows: Vec<EditorWindow>,
}

/// An editor window's title: "<plugin> · <strip> · slot N", or `None` when the setup has no
/// such insert.
pub fn title(setup: &Setup, instance: InstanceId) -> Option<String> {
    let strips = setup
        .channels
        .iter()
        .map(|c| (Strip::Channel(c.id), c.name.as_str(), &c.chain))
        .chain([MixId::Monitor, MixId::Master].map(|m| {
            let mix = setup.mix(m);
            (Strip::Mix(m), m.name(), &mix.chain)
        }));
    for (_, name, chain) in strips {
        if let Some(slot) = chain.iter().position(|i| i.instance == instance) {
            let plugin = &chain[slot].plugin.name;
            return Some(format!("{plugin} \u{b7} {name} \u{b7} slot {}", slot + 1));
        }
    }
    None
}

impl EditorWindows {
    /// Whether no editor is open.
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// Opens `instance`'s editor in a window of its own, or raises the one it has. The error
    /// says why it could not, and what to do.
    pub fn open(
        &mut self,
        event_loop: &ActiveEventLoop,
        host: &mut dyn Host,
        instance: InstanceId,
    ) -> Result<(), String> {
        if let Some(open) = self.windows.iter().find(|w| w.instance == instance) {
            open.window.focus_window();
            return Ok(());
        }
        let Some(title) = title(host.setup(), instance) else {
            return Err(
                "that insert is gone (it may have just been removed): open the editor of one the console shows"
                    .to_string(),
            );
        };
        if self.windows.len() >= MAX_EDITORS {
            return Err(format!(
                "{MAX_EDITORS} plugin windows are open, the most there can be: close one, then open {title}"
            ));
        }
        // Hidden until the plugin says how big it is, so it never flashes at a guess.
        let attributes = Window::default_attributes()
            .with_title(&title)
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(winit::dpi::PhysicalSize::new(FIRST_SIZE[0], FIRST_SIZE[1]));
        let window = event_loop.create_window(attributes).map_err(|e| {
            format!("no window for {title} ({e}): close other windows and try again")
        })?;
        let handle = window.window_handle().map_err(|e| {
            format!("the window for {title} has no handle a plugin can draw into ({e})")
        })?;
        let size = host
            .open_editor(instance, handle.as_raw())
            .map_err(|e| format!("{title}: {e}"))?;
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(size.width, size.height));
        window.set_visible(true);
        log::info!(
            "opened the editor of {title} at {} x {}",
            size.width,
            size.height
        );
        self.windows.push(EditorWindow {
            instance,
            window,
            title,
        });
        Ok(())
    }

    /// Closes `instance`'s editor through the host, then its window.
    pub fn close(&mut self, host: &mut dyn Host, instance: InstanceId) {
        host.close_editor(instance);
        if let Some(open) = self.windows.iter().find(|w| w.instance == instance) {
            log::info!("closed the editor of {}", open.title);
        }
        self.forget(instance);
    }

    /// Closes every editor through the host, then the windows.
    pub fn close_all(&mut self, host: &mut dyn Host) {
        for open in self.windows.drain(..) {
            host.close_editor(open.instance);
        }
    }

    /// Drops `instance`'s window: the host has closed its editor already.
    pub fn forget(&mut self, instance: InstanceId) {
        self.windows.retain(|w| w.instance != instance);
    }

    /// Serves the open editors: what each asked of its window since the last frame.
    pub fn tick(&mut self, host: &mut dyn Host) {
        if self.windows.is_empty() {
            return;
        }
        for (instance, event) in host.tick_editors() {
            let Some(open) = self.windows.iter().find(|w| w.instance == instance) else {
                continue;
            };
            match event {
                EditorEvent::ResizeRequested(size) => {
                    log::info!(
                        "the editor of {} asks for {} x {}",
                        open.title,
                        size.width,
                        size.height
                    );
                    let _ = open
                        .window
                        .request_inner_size(winit::dpi::PhysicalSize::new(size.width, size.height));
                }
            }
        }
    }

    /// Renames the windows whose insert moved or whose strip was renamed, and closes the
    /// ones whose insert left the setup.
    pub fn follow(&mut self, host: &mut dyn Host) {
        let mut gone = Vec::new();
        for open in &mut self.windows {
            match title(host.setup(), open.instance) {
                Some(title) if title != open.title => {
                    open.window.set_title(&title);
                    open.title = title;
                }
                Some(_) => {}
                None => gone.push(open.instance),
            }
        }
        for instance in gone {
            self.close(host, instance);
        }
    }

    /// An event for one of the editor windows; `false` when `id` is none of them. Closing
    /// the window closes its editor first.
    pub fn window_event(&mut self, host: &mut dyn Host, id: WindowId, event: &WindowEvent) -> bool {
        let Some(open) = self.windows.iter().find(|w| w.window.id() == id) else {
            return false;
        };
        if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
            let instance = open.instance;
            self.close(host, instance);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::example;

    #[test]
    fn an_editor_window_is_titled_plugin_strip_and_slot() {
        let setup = example::setup();
        let mic = &setup.channels[0];
        assert_eq!(
            title(&setup, mic.chain[1].instance).as_deref(),
            Some("Pro-C 2 \u{b7} Mic \u{b7} slot 2")
        );
        let limiter = setup.master.chain[1].instance;
        assert_eq!(
            title(&setup, limiter).as_deref(),
            Some("Pro-L 2 \u{b7} Master \u{b7} slot 2")
        );
        assert_eq!(title(&setup, InstanceId(9_999)), None);
    }
}
