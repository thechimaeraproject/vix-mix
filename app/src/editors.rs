//! Plugin editor windows: one window per open editor, which the plugin draws into itself
//! (egui never does), titled "<plugin> · <strip> · slot N". Vix's `open_editor` and
//! `EditorWindow` are the model.
//!
//! - **Open**: a window is made hidden and fixed-size, its raw handle goes to
//!   `Host::open_editor`, and the window takes the size the plugin asks for and shows. A
//!   refusal (no editor, `FakeHost`, a Wayland handle on Linux) drops the window and says why.
//!   Opening an editor that is open raises its window.
//! - **Resize**: `Host::tick_editors` names the instance that asked; its window is resized.
//! - **Close**: always through the host before the window goes — the window's close button,
//!   the insert's removal (the shell closes it before the command), the insert leaving the
//!   setup any other way, the window closing — and an editor the host closed itself
//!   (`HostEvent::EditorClosed`) loses its window.
//! - **Linux**: the event loop runs on X11 (XWayland under KDE Wayland), because the kit
//!   refuses Wayland handles (`main.rs`). The host polls each editor's run loop in
//!   `tick_editors`, which the frame calls at least every [`EDITOR_POLL`].
//!
//! The windows are made by an [`Opener`] and driven through [`EditorWindow`]: winit's event
//! loop and window in the app, a fake in the tests, so every path above is tested headless.

use std::time::Duration;

use chimaeraproject_vst3_host::{EditorEvent, EditorSize};
use vixmix_host::Host;
use vixmix_model::{InstanceId, MixId, Setup, Strip};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

/// How often the frame comes round while an editor is open: a plugin's window is served
/// (its Linux run loop, its resize requests) only when the host is asked.
pub const EDITOR_POLL: Duration = Duration::from_millis(33);

/// The most editor windows open at once; a further one is refused with a toast.
pub const MAX_EDITORS: usize = 16;

/// The size an editor's window is made at, hidden, before its plugin says what it wants.
pub const FIRST_SIZE: [u32; 2] = [640, 480];

/// What the editors need of a window a plugin draws into.
pub trait EditorWindow {
    /// The id its events come with.
    fn id(&self) -> WindowId;
    /// The handle a plugin's editor is opened into; why not, when it has none.
    fn handle(&self) -> Result<RawWindowHandle, String>;
    /// Takes this size, in pixels.
    fn resize(&self, size: EditorSize);
    /// Shows it.
    fn show(&self);
    /// Titles it.
    fn set_title(&self, title: &str);
    /// Raises it and gives it the focus.
    fn raise(&self);
}

/// Makes the editors' windows.
pub trait Opener {
    /// A window titled `title`, hidden, fixed-size, `size` pixels; why not, when it cannot.
    fn create(&self, title: &str, size: [u32; 2]) -> Result<Box<dyn EditorWindow>, String>;
}

impl Opener for ActiveEventLoop {
    fn create(&self, title: &str, size: [u32; 2]) -> Result<Box<dyn EditorWindow>, String> {
        let attributes = Window::default_attributes()
            .with_title(title)
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(winit::dpi::PhysicalSize::new(size[0], size[1]));
        let window = self.create_window(attributes).map_err(|e| e.to_string())?;
        Ok(Box::new(window))
    }
}

impl EditorWindow for Window {
    fn id(&self) -> WindowId {
        Window::id(self)
    }

    fn handle(&self) -> Result<RawWindowHandle, String> {
        self.window_handle()
            .map(|handle| handle.as_raw())
            .map_err(|e| e.to_string())
    }

    fn resize(&self, size: EditorSize) {
        let _ = self.request_inner_size(winit::dpi::PhysicalSize::new(size.width, size.height));
    }

    fn show(&self) {
        self.set_visible(true);
    }

    fn set_title(&self, title: &str) {
        Window::set_title(self, title);
    }

    fn raise(&self) {
        self.focus_window();
    }
}

/// One open editor.
struct Open {
    instance: InstanceId,
    window: Box<dyn EditorWindow>,
    title: String,
}

/// Every open editor's window.
#[derive(Default)]
pub struct EditorWindows {
    windows: Vec<Open>,
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

    /// The instances whose editors are open, in the order they opened.
    #[cfg(test)]
    pub fn open_instances(&self) -> Vec<InstanceId> {
        self.windows.iter().map(|w| w.instance).collect()
    }

    /// Opens `instance`'s editor in a window of its own, or raises the one it has. The error
    /// says why it could not, and what to do; no window is kept for a refusal.
    pub fn open(
        &mut self,
        opener: &dyn Opener,
        host: &mut dyn Host,
        instance: InstanceId,
    ) -> Result<(), String> {
        if let Some(open) = self.windows.iter().find(|w| w.instance == instance) {
            open.window.raise();
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
        let window = opener.create(&title, FIRST_SIZE).map_err(|e| {
            format!("no window for {title} ({e}): close other windows and try again")
        })?;
        let handle = window.handle().map_err(|e| {
            format!(
                "the window for {title} has no handle a plugin can draw into ({e}): close it and open the editor again"
            )
        })?;
        let size = host
            .open_editor(instance, handle)
            .map_err(|e| format!("{title}: {e}"))?;
        window.resize(size);
        window.show();
        log::info!(
            "opened the editor of {title} at {} x {}",
            size.width,
            size.height
        );
        self.windows.push(Open {
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

    /// Closes every editor through the host, in the order they opened, then the windows.
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
                    open.window.resize(size);
                }
            }
        }
    }

    /// Renames the windows whose insert moved or whose strip was renamed, and closes (through
    /// the host) the ones whose insert left the setup.
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
    use crate::test_support::{Call, FakeWindows, Recorder, WindowCall};
    use vixmix_model::{Command, example};

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

    #[test]
    fn an_editor_opens_hidden_then_takes_the_plugins_size_and_shows() {
        let mut host = Recorder::with_editors();
        let windows = FakeWindows::default();
        let mut editors = EditorWindows::default();
        let insert = host.setup().channels[0].chain[1].instance;
        editors.open(&windows, &mut host, insert).unwrap();
        assert_eq!(host.calls, vec![Call::OpenEditor(insert)]);
        assert_eq!(
            windows.calls(),
            vec![
                WindowCall::Create {
                    window: 0,
                    title: "Pro-C 2 \u{b7} Mic \u{b7} slot 2".to_string(),
                    size: FIRST_SIZE,
                },
                WindowCall::Resize(0, Recorder::EDITOR_SIZE),
                WindowCall::Show(0),
            ]
        );
        // Opened again: raised, not opened twice.
        editors.open(&windows, &mut host, insert).unwrap();
        assert_eq!(host.calls.len(), 1);
        assert_eq!(windows.calls().last(), Some(&WindowCall::Raise(0)));
        assert_eq!(editors.open_instances(), vec![insert]);
    }

    #[test]
    fn a_refused_editor_keeps_no_window_and_says_what_to_do() {
        // FakeHost has no plugin instances: its refusal says to run the real host.
        let mut host = Recorder::new();
        let windows = FakeWindows::default();
        let mut editors = EditorWindows::default();
        let insert = host.setup().channels[0].chain[0].instance;
        let refused = editors.open(&windows, &mut host, insert).unwrap_err();
        assert!(
            refused.starts_with("Noise Suppression for Voice \u{b7} Mic \u{b7} slot 1: "),
            "{refused}"
        );
        assert!(refused.contains("run with the real host"), "{refused}");
        assert!(editors.is_empty());
        assert_eq!(windows.calls().len(), 1, "made, then dropped unshown");
        // A window the desktop will not make, and one with no handle, say what to do, and
        // the host is never asked.
        let mut host = Recorder::with_editors();
        let refused = editors
            .open(&FakeWindows::refusing(), &mut host, insert)
            .unwrap_err();
        assert!(
            refused.contains("close other windows and try again"),
            "{refused}"
        );
        let refused = editors
            .open(&FakeWindows::without_handles(), &mut host, insert)
            .unwrap_err();
        assert!(refused.contains("open the editor again"), "{refused}");
        assert!(editors.is_empty());
        assert!(host.calls.is_empty());
        // An insert that is gone.
        let refused = editors
            .open(&windows, &mut host, InstanceId(9_999))
            .unwrap_err();
        assert!(refused.contains("open the editor of one the console shows"));
    }

    #[test]
    fn at_most_sixteen_editors_open() {
        let mut host = Recorder::with_editors();
        let windows = FakeWindows::default();
        let mut editors = EditorWindows::default();
        let monitor = Strip::Mix(MixId::Monitor);
        let plugin = host.setup().channels[0].chain[0].plugin.clone();
        while host.setup().monitor.chain.len() < MAX_EDITORS + 1 {
            let at = host.setup().monitor.chain.len();
            let added = host.apply(Command::AddInsert {
                strip: monitor,
                plugin: plugin.clone(),
                at,
            });
            assert!(added.unwrap().changed());
        }
        let chain: Vec<InstanceId> = host
            .setup()
            .monitor
            .chain
            .iter()
            .map(|i| i.instance)
            .collect();
        for instance in &chain[..MAX_EDITORS] {
            editors.open(&windows, &mut host, *instance).unwrap();
        }
        let refused = editors
            .open(&windows, &mut host, chain[MAX_EDITORS])
            .unwrap_err();
        assert!(
            refused.starts_with("16 plugin windows are open"),
            "{refused}"
        );
        assert!(refused.contains("close one, then open"), "{refused}");
        assert_eq!(editors.open_instances().len(), MAX_EDITORS);
    }

    #[test]
    fn a_resize_request_goes_to_the_window_of_the_editor_that_asked() {
        let mut host = Recorder::with_editors();
        let windows = FakeWindows::default();
        let mut editors = EditorWindows::default();
        let [a, b] = [0, 1].map(|i| host.setup().channels[0].chain[i].instance);
        editors.open(&windows, &mut host, a).unwrap();
        editors.open(&windows, &mut host, b).unwrap();
        let before = windows.calls().len();
        let size = EditorSize {
            width: 800,
            height: 300,
        };
        host.editor_events = vec![(b, EditorEvent::ResizeRequested(size))];
        editors.tick(&mut host);
        assert_eq!(windows.calls()[before..], [WindowCall::Resize(1, size)]);
        // An event for an instance with no window is ignored.
        host.editor_events = vec![(InstanceId(9_999), EditorEvent::ResizeRequested(size))];
        editors.tick(&mut host);
        assert_eq!(windows.calls().len(), before + 1);
    }

    #[test]
    fn closing_an_editors_window_closes_the_editor_through_the_host_first() {
        let mut host = Recorder::with_editors();
        let windows = FakeWindows::default();
        let mut editors = EditorWindows::default();
        let [a, b] = [0, 1].map(|i| host.setup().channels[0].chain[i].instance);
        editors.open(&windows, &mut host, a).unwrap();
        editors.open(&windows, &mut host, b).unwrap();
        host.calls.clear();
        // An event for a window that is no editor's is not taken.
        let focused = WindowEvent::Focused(true);
        assert!(!editors.window_event(&mut host, WindowId::from(99), &focused));
        // b's close button.
        let close = WindowEvent::CloseRequested;
        assert!(editors.window_event(&mut host, FakeWindows::id(1), &close));
        assert_eq!(host.calls, vec![Call::CloseEditor(b)]);
        assert_eq!(editors.open_instances(), vec![a]);
    }

    #[test]
    fn an_editor_the_host_closed_loses_its_window_without_a_second_close() {
        let mut host = Recorder::with_editors();
        let windows = FakeWindows::default();
        let mut editors = EditorWindows::default();
        let mut shell = crate::shell::Shell::default();
        let a = host.setup().channels[0].chain[0].instance;
        editors.open(&windows, &mut host, a).unwrap();
        host.calls.clear();
        shell.events(
            vec![vixmix_host::HostEvent::EditorClosed(a)],
            0.0,
            &host,
            &mut editors,
        );
        assert!(editors.is_empty());
        assert!(host.calls.is_empty(), "the host closed it already");
    }

    #[test]
    fn an_editor_follows_its_insert_and_closes_when_it_leaves_the_setup() {
        let mut host = Recorder::with_editors();
        let windows = FakeWindows::default();
        let mut editors = EditorWindows::default();
        let mic = host.setup().channels[0].id;
        let [a, b] = [0, 1].map(|i| host.setup().channels[0].chain[i].instance);
        editors.open(&windows, &mut host, a).unwrap();
        editors.open(&windows, &mut host, b).unwrap();
        // The strip renamed: the titles follow.
        let renamed = host.apply(Command::Rename {
            ch: mic,
            name: "Vocal".to_string(),
        });
        assert!(renamed.unwrap().changed());
        let before = windows.calls().len();
        editors.follow(&mut host);
        assert_eq!(
            windows.calls()[before..],
            [
                WindowCall::Title(
                    0,
                    "Noise Suppression for Voice \u{b7} Vocal \u{b7} slot 1".to_string()
                ),
                WindowCall::Title(1, "Pro-C 2 \u{b7} Vocal \u{b7} slot 2".to_string()),
            ]
        );
        // The insert removed other than through the window (the host's own doing): its
        // editor closes through the host at the next frame, and b, a slot up, is renamed.
        let removed = host.inner.apply(Command::RemoveInsert { instance: a });
        assert!(removed.unwrap().changed());
        host.calls.clear();
        editors.follow(&mut host);
        assert_eq!(host.calls, vec![Call::CloseEditor(a)]);
        assert_eq!(editors.open_instances(), vec![b]);
        assert_eq!(
            windows.calls().last(),
            Some(&WindowCall::Title(
                1,
                "Pro-C 2 \u{b7} Vocal \u{b7} slot 1".to_string()
            ))
        );
    }

    #[test]
    fn at_exit_every_editor_closes_through_the_host_in_the_order_it_opened() {
        let mut host = Recorder::with_editors();
        let windows = FakeWindows::default();
        let mut editors = EditorWindows::default();
        let opened = [
            host.setup().channels[0].chain[1].instance,
            host.setup().master.chain[0].instance,
            host.setup().channels[0].chain[0].instance,
        ];
        for instance in opened {
            editors.open(&windows, &mut host, instance).unwrap();
        }
        host.calls.clear();
        editors.close_all(&mut host);
        assert_eq!(host.calls, opened.map(Call::CloseEditor));
        assert!(editors.is_empty());
    }
}
