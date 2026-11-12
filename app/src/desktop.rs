//! What the desktop knows a window of ours by: its name, which files it under Vix Mix's entry
//! in the task manager with its icon, and the activation token the launcher handed the
//! process, which ends the launch feedback and gives the new window the focus.
//!
//! **The name.** The desktop entry is `packaging/linux/org.chimaeraproject.vixmix.desktop`.
//! A Wayland window finds it by its app id, which must be the entry's file name ([`APP_ID`]);
//! an X11 window by its class, which the entry's `StartupWMClass` names ([`WM_CLASS`]). winit
//! sets no app id unless told, and takes the X11 class from the program's file name, so both
//! are said here rather than left to how the binary was installed.
//!
//! **The token.** A launcher hands it in the environment: `DESKTOP_STARTUP_ID` to an X11
//! window, `XDG_ACTIVATION_TOKEN` to a Wayland one. winit reads the one its event loop can
//! use, and the window made with it tells the desktop the launch is over. Both variables are
//! then removed, so no child process (the plugin scan's, a plugin's own) takes the token for
//! its own. Under a Wayland session an XWayland window (ours, whenever there is an X display)
//! is handed only the Wayland token, which it cannot use: there the desktop ends the feedback
//! itself, when the window of the entry's class appears.
//!
//! Linux only; elsewhere each function hands back what it was given.

use winit::event_loop::ActiveEventLoop;
use winit::window::{ActivationToken, WindowAttributes};

/// The desktop entry's file name without `.desktop`, which is also the AppStream id and the
/// icon's name: the app id of a Wayland window.
#[cfg(any(target_os = "linux", test))]
pub const APP_ID: &str = "org.chimaeraproject.vixmix";

/// The class of an X11 window: the desktop entry's `StartupWMClass`.
#[cfg(any(target_os = "linux", test))]
pub const WM_CLASS: &str = "vix-mix";

/// The general and the instance name winit is given. On Wayland the general name is the app
/// id and the instance is unused; on X11 they are the class and the instance of `WM_CLASS`.
#[cfg(any(target_os = "linux", test))]
fn names(wayland: bool) -> (&'static str, &'static str) {
    if wayland {
        (APP_ID, WM_CLASS)
    } else {
        (WM_CLASS, WM_CLASS)
    }
}

/// `attributes` named for the desktop the event loop runs on: the console's window and every
/// editor's, so the task manager groups them under one entry.
pub fn named(attributes: WindowAttributes, event_loop: &ActiveEventLoop) -> WindowAttributes {
    #[cfg(target_os = "linux")]
    {
        // winit keeps one name for both; the X11 trait's `with_name` sets the same one.
        use winit::platform::wayland::{ActiveEventLoopExtWayland, WindowAttributesExtWayland};
        let (general, instance) = names(event_loop.is_wayland());
        attributes.with_name(general, instance)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = event_loop;
        attributes
    }
}

/// The launcher's activation token for this event loop, if it handed one, taken out of the
/// environment for good. Call it before anything starts a child process.
pub fn take_activation_token(event_loop: &ActiveEventLoop) -> Option<ActivationToken> {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::startup_notify::{
            EventLoopExtStartupNotify, reset_activation_token_env,
        };
        let token = event_loop.read_token_from_env();
        reset_activation_token_env();
        token
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = event_loop;
        None
    }
}

/// `attributes` for the window the launch was for: made with `token`, it ends the launch
/// feedback and takes the focus.
pub fn launched(attributes: WindowAttributes, token: Option<ActivationToken>) -> WindowAttributes {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::startup_notify::WindowAttributesExtStartupNotify;
        match token {
            Some(token) => attributes.with_activation_token(token),
            None => attributes,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = token;
        attributes
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn the_desktop_entry_and_the_windows_name_each_other() {
        // The entry is found under the Wayland app id: its file name is the id.
        let entry = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../packaging/linux")
            .join(format!("{APP_ID}.desktop"));
        let text = std::fs::read_to_string(&entry).unwrap_or_else(|e| {
            panic!(
                "{}: {e}. A Wayland window finds its desktop entry by its app id, so the entry's file name and `APP_ID` move together",
                entry.display()
            )
        });
        let value = |key: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        };
        assert_eq!(names(true).0, APP_ID, "a Wayland window's app id");
        // An X11 window is found by its class.
        assert_eq!(names(false), (WM_CLASS, WM_CLASS));
        assert_eq!(value("StartupWMClass"), Some(WM_CLASS));
        assert_eq!(value("Icon"), Some(APP_ID));
        // The window is made with the launcher's token, which is what `true` promises.
        assert_eq!(value("StartupNotify"), Some("true"));
    }
}
