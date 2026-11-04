//! [`EditorSession`]: one plugin's editor in a window the app made. The app owns the window
//! (winit, on the X11 backend on Linux, because the kit refuses Wayland handles); this owns
//! the plugin's view inside it, on the UI thread.

use chimaeraproject_vst3_host::{Editor, EditorEvent, EditorSize, RunLoop, Vst3Editor};
use raw_window_handle::RawWindowHandle;
use vixmix_model::InstanceId;

use crate::instances::PluginError;

/// An open (or openable) editor for one instance.
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

    /// Attaches the editor to `parent` (a Win32, Xlib or Xcb child window) and returns the
    /// size it wants the window to be.
    pub fn open(&mut self, parent: RawWindowHandle) -> Result<EditorSize, PluginError> {
        self.editor
            .open(parent)
            .map_err(|source| PluginError::Vst3 {
                name: format!("the editor of instance {:?}", self.instance),
                source,
            })
    }

    /// What the editor asked of its window since the last call: resize the window, then
    /// call [`resize`](EditorSession::resize).
    pub fn poll_events(&mut self) -> Vec<EditorEvent> {
        self.editor.poll_events()
    }

    /// Tells the editor its window is now `size`.
    pub fn resize(&mut self, size: EditorSize) -> Result<(), PluginError> {
        self.editor
            .resize(size)
            .map_err(|source| PluginError::Vst3 {
                name: format!("the editor of instance {:?}", self.instance),
                source,
            })
    }

    /// The run loop the plugin registered (Linux): poll it every UI tick. `None` elsewhere.
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
