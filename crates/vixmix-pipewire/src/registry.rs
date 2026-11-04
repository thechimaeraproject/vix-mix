//! What the registry's globals mean to the backend: devices that are not ours, app playback
//! streams with the identity the host routes by, and the `default` metadata routing writes
//! to. Pure functions over property dictionaries, so they are tested without a daemon.

use pipewire::spa::utils::dict::DictRef;
use pipewire::types::ObjectType;
use vixmix_model::{AppRef, DeviceDirection, DeviceInfo, DeviceRef};

use crate::AppStream;

/// What one global is to the backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Global {
    /// An audio device (a sink or a source) that is not one of our nodes.
    Device(DeviceInfo),
    /// Another app's playback stream: bind it for its full properties (the registry's
    /// global carries only a few of them).
    AppStream,
    /// The `default` metadata object: where `target.object` routing goes.
    DefaultMetadata,
    /// Anything else, ours included.
    Other,
}

/// Sorts a registry global. `app` is our `application.name` and node-name prefix: every node
/// named `<app>.…` is ours and never reported.
pub(crate) fn classify(type_: &ObjectType, props: Option<&DictRef>, app: &str) -> Global {
    let Some(props) = props else {
        return Global::Other;
    };
    match type_ {
        ObjectType::Node => {
            let name = props.get("node.name").unwrap_or_default();
            if is_ours(name, app) {
                return Global::Other;
            }
            let direction = match props.get("media.class") {
                Some("Audio/Sink") => DeviceDirection::Output,
                // A virtual source (a null source, a loopback's capture side) records like a
                // device: PipeWire's own null source is `Audio/Source/Virtual`.
                Some("Audio/Source" | "Audio/Source/Virtual") => DeviceDirection::Input,
                Some("Stream/Output/Audio") => return Global::AppStream,
                _ => return Global::Other,
            };
            if name.is_empty() {
                return Global::Other;
            }
            let description = props
                .get("node.description")
                .or_else(|| props.get("node.nick"))
                .filter(|d| !d.is_empty())
                .unwrap_or(name);
            Global::Device(DeviceInfo {
                device: DeviceRef {
                    name: name.to_string(),
                    description: description.to_string(),
                },
                direction,
            })
        }
        ObjectType::Metadata if props.get("metadata.name") == Some("default") => {
            Global::DefaultMetadata
        }
        _ => Global::Other,
    }
}

/// Whether `node_name` is one of ours: `<app>.` followed by anything.
pub(crate) fn is_ours(node_name: &str, app: &str) -> bool {
    node_name
        .strip_prefix(app)
        .is_some_and(|rest| rest.starts_with('.'))
}

/// An app stream's identity from its node's full properties: the portal app id, the process
/// binary and `application.name` (the node's name when an app sets none), and what it plays.
pub(crate) fn app_stream(node: u32, props: &DictRef) -> AppStream {
    let text = |key: &str| props.get(key).filter(|v| !v.is_empty()).map(str::to_string);
    AppStream {
        node,
        app: AppRef {
            app_id: text("pipewire.access.portal.app_id"),
            binary: text("application.process.binary"),
            name: text("application.name")
                .or_else(|| text("node.name"))
                .unwrap_or_else(|| format!("node {node}")),
        },
        media_name: text("media.name"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pipewire::properties::properties;

    #[test]
    fn sinks_and_sources_that_are_not_ours_are_devices_with_their_direction() {
        let sink = properties! {
            "node.name" => "vixmix-ci-null-sink",
            "node.description" => "vix-mix CI null sink",
            "media.class" => "Audio/Sink",
        };
        assert_eq!(
            classify(&ObjectType::Node, Some(sink.dict()), "vix-mix"),
            Global::Device(DeviceInfo {
                device: DeviceRef {
                    name: "vixmix-ci-null-sink".into(),
                    description: "vix-mix CI null sink".into(),
                },
                direction: DeviceDirection::Output,
            })
        );
        let source = properties! {
            "node.name" => "vixmix-ci-null-source",
            "media.class" => "Audio/Source/Virtual",
        };
        let Global::Device(info) = classify(&ObjectType::Node, Some(source.dict()), "vix-mix")
        else {
            panic!("a virtual source is a device");
        };
        assert_eq!(info.direction, DeviceDirection::Input);
        assert_eq!(info.device.description, "vixmix-ci-null-source");
    }

    #[test]
    fn our_own_nodes_are_never_devices_or_apps() {
        for (name, class) in [
            ("vix-mix.ch1.music", "Audio/Sink"),
            ("vix-mix.master", "Audio/Source"),
            ("vix-mix.monitor-out", "Stream/Output/Audio"),
        ] {
            let props = properties! { "node.name" => name, "media.class" => class };
            assert_eq!(
                classify(&ObjectType::Node, Some(props.dict()), "vix-mix"),
                Global::Other,
                "{name}"
            );
        }
        assert!(!is_ours("vix-mixer.sink", "vix-mix"));
        assert!(is_ours("vix-mix.anything", "vix-mix"));
    }

    #[test]
    fn an_app_stream_carries_the_three_identities_and_what_it_plays() {
        let props = properties! {
            "media.class" => "Stream/Output/Audio",
            "node.name" => "Firefox",
            "application.name" => "Firefox",
            "application.process.binary" => "firefox",
            "pipewire.access.portal.app_id" => "org.mozilla.firefox",
            "media.name" => "A video",
        };
        assert_eq!(
            classify(&ObjectType::Node, Some(props.dict()), "vix-mix"),
            Global::AppStream
        );
        assert_eq!(
            app_stream(42, props.dict()),
            AppStream {
                node: 42,
                app: AppRef {
                    app_id: Some("org.mozilla.firefox".into()),
                    binary: Some("firefox".into()),
                    name: "Firefox".into(),
                },
                media_name: Some("A video".into()),
            }
        );
        let bare = properties! { "node.name" => "pw-play" };
        assert_eq!(app_stream(7, bare.dict()).app.name, "pw-play");
    }

    #[test]
    fn only_the_metadata_named_default_is_the_routing_target() {
        let default = properties! { "metadata.name" => "default" };
        let settings = properties! { "metadata.name" => "settings" };
        assert_eq!(
            classify(&ObjectType::Metadata, Some(default.dict()), "vix-mix"),
            Global::DefaultMetadata
        );
        assert_eq!(
            classify(&ObjectType::Metadata, Some(settings.dict()), "vix-mix"),
            Global::Other
        );
    }
}
