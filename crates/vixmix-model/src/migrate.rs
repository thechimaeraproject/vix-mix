//! The older formats of `setup.ron`, each read with its own types and converted one step
//! forward, so a file any vix-mix wrote still opens. A step reuses the current types for the
//! parts its format shares with the next; when a later format changes one of those, copy its
//! old shape in here before changing it.
//!
//! Format 1, the bootstrap's, had no id counters, kept `version` as its last field, and wrote
//! a plugin's state as a list of numbers.

use serde::Deserialize;

use crate::ids::{ChannelId, InstanceId, MixId};
use crate::persist::{PersistError, VERSION};
use crate::setup::{self, DeviceRef, PluginRef, Prefs, Send, Source};

/// Reads format 1 and converts it to the current format. Format 2 is the current one, so this
/// is one step; a format 3 would make it `v2::Setup::from(v1)` and then the step to 3.
pub(crate) fn from_v1(text: &str) -> Result<setup::Setup, PersistError> {
    let old: V1Setup = ron::from_str(text)?;
    let mut new = setup::Setup {
        version: VERSION,
        // Format 1 handed out one more than the largest id in use; raising the counters
        // above every id in the file is the same rule from here on.
        next_channel: ChannelId(1),
        next_instance: InstanceId(1),
        channels: old
            .channels
            .into_iter()
            .map(V1Channel::into_current)
            .collect(),
        monitor: old.monitor.into_current(),
        master: old.master.into_current(),
        preview_master: old.preview_master,
        prefs: old.prefs,
    };
    new.raise_counters();
    Ok(new)
}

#[derive(Deserialize)]
struct V1Setup {
    channels: Vec<V1Channel>,
    monitor: V1Mix,
    master: V1Mix,
    preview_master: bool,
    prefs: Prefs,
}

#[derive(Deserialize)]
struct V1Channel {
    id: ChannelId,
    name: String,
    source: Source,
    chain: Vec<V1Insert>,
    chain_to_monitor: bool,
    chain_to_master: bool,
    monitor: Send,
    master: Send,
    link: bool,
}

impl V1Channel {
    fn into_current(self) -> setup::Channel {
        setup::Channel {
            id: self.id,
            name: self.name,
            source: self.source,
            chain: self.chain.into_iter().map(V1Insert::into_current).collect(),
            chain_to_monitor: self.chain_to_monitor,
            chain_to_master: self.chain_to_master,
            monitor: self.monitor,
            master: self.master,
            link: self.link,
        }
    }
}

#[derive(Deserialize)]
struct V1Mix {
    id: MixId,
    device: Option<DeviceRef>,
    level: Send,
    chain: Vec<V1Insert>,
}

impl V1Mix {
    fn into_current(self) -> setup::Mix {
        setup::Mix {
            id: self.id,
            device: self.device,
            level: self.level,
            chain: self.chain.into_iter().map(V1Insert::into_current).collect(),
        }
    }
}

/// Format 1's insert: the state as RON's list of numbers.
#[derive(Deserialize)]
struct V1Insert {
    instance: InstanceId,
    plugin: PluginRef,
    enabled: bool,
    state: Option<Vec<u8>>,
}

impl V1Insert {
    fn into_current(self) -> setup::Insert {
        setup::Insert {
            instance: self.instance,
            plugin: self.plugin,
            enabled: self.enabled,
            state: self.state,
        }
    }
}
