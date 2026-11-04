//! [`EditorSession`]: one plugin's editor in a window the app made. The app owns the window
//! (winit, on the X11 backend on Linux, because the kit refuses Wayland handles); this owns
//! the plugin's view inside it, on the UI thread.

use chimaeraproject_vst3_host::{Editor, EditorEvent, EditorSize, RunLoop, Vst3Editor};
use raw_window_handle::RawWindowHandle;
use vixmix_model::InstanceId;

use crate::instances::PluginError;

/// An open (or openable) editor for one instance. Close it before its window goes and before
/// its instance is retired.
pub struct EditorSession {
    instance: InstanceId,
    editor: Vst3Editor,
}

impl EditorSession {
    pub(crate) fn new(instance: InstanceId, editor: Vst3Editor) -> EditorSession {
        EditorSession { instance, editor }
    }

    /// The instance this editor shows.
    pub fn instance(&self) -> InstanceId {
        self.instance
    }

    fn error(&self, source: chimaeraproject_vst3_host::Vst3Error) -> PluginError {
        PluginError::Vst3 {
            name: format!("the editor of instance {:?}", self.instance),
            source,
        }
    }

    /// Attaches the editor to `parent` (a Win32, Xlib or Xcb child window; a Wayland handle is
    /// refused with the fix) and returns the size it wants the window to be.
    pub fn open(&mut self, parent: RawWindowHandle) -> Result<EditorSize, PluginError> {
        self.editor
            .open(parent)
            .map_err(|source| self.error(source))
    }

    /// One UI tick: on Linux the plugin's run loop is served first (its timers and the
    /// descriptors it registered), then what the editor asked of its window since the last
    /// tick is returned. For `ResizeRequested`, resize the window, then call
    /// [`resize`](EditorSession::resize).
    pub fn tick(&mut self) -> Vec<EditorEvent> {
        if let Some(run_loop) = self.editor.run_loop() {
            run_loop.poll();
        }
        self.editor.poll_events()
    }

    /// What the editor asked of its window since the last call, without serving the run
    /// loop; [`tick`](EditorSession::tick) does both.
    pub fn poll_events(&mut self) -> Vec<EditorEvent> {
        self.editor.poll_events()
    }

    /// Whether the user may resize the editor's window.
    pub fn can_resize(&self) -> bool {
        self.editor.can_resize()
    }

    /// The nearest size the editor accepts to `size`: what a window the user is dragging
    /// should snap to.
    pub fn check_size(&self, size: EditorSize) -> EditorSize {
        self.editor.check_size(size)
    }

    /// The size the editor reports now.
    pub fn size(&self) -> Option<EditorSize> {
        self.editor.size()
    }

    /// Tells the editor its window is now `size`.
    pub fn resize(&mut self, size: EditorSize) -> Result<(), PluginError> {
        self.editor
            .resize(size)
            .map_err(|source| self.error(source))
    }

    /// Tells the editor the display's scale factor; `false` when it does not scale.
    pub fn set_scale_factor(&mut self, factor: f32) -> bool {
        self.editor.set_scale_factor(factor)
    }

    /// The run loop the plugin registered (Linux), for a caller that serves it itself.
    /// `None` elsewhere.
    pub fn run_loop(&self) -> Option<RunLoop> {
        self.editor.run_loop()
    }

    /// Whether the editor is attached to a window.
    pub fn is_open(&self) -> bool {
        self.editor.is_open()
    }

    /// Detaches the editor. Close it before its window goes.
    pub fn close(&mut self) {
        self.editor.close();
    }
}
