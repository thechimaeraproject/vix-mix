//! Every change to a [`Setup`] is a [`Command`] passed to [`apply`], which validates it,
//! applies it whole or not at all, and says in [`Applied`] exactly what it changed, so the host
//! knows whether to prepare the engine again, apply the backend's graph, route apps, create or
//! retire plugin instances, or only save. Each part of `Applied` is set exactly when its part
//! of the setup changed: a command that changes nothing (a drag dropped where it started, a
//! mute set to what it already was) answers [`Applied::default`].

use std::mem::replace;

use crate::ids::{ChannelId, InstanceId, MixId};
use crate::setup::{
    AppRef, Channel, ChannelKind, DeviceRef, Insert, MAX_CHANNELS, PluginRef, Prefs, Send, Setup,
    Source,
};
use crate::snapshot;

/// A strip that holds a chain: a channel, or one of the mixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Strip {
    Channel(ChannelId),
    Mix(MixId),
}

/// A place in a strip's chain. As a source it names an insert (`index < len`); as a
/// destination it names the gap before `index` (`index <= len`), where the drop line is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotRef {
    pub strip: Strip,
    pub index: usize,
}

/// One change to the setup, as the UI sends it.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// The add-channel menu: a new channel of `kind` at the right end, sends at unity, linked.
    AddChannel { kind: ChannelKind },
    /// Remove a channel; its inserts' instances are retired.
    RemoveChannel(ChannelId),
    /// The name field of the source popover.
    Rename { ch: ChannelId, name: String },
    /// The source popover: a device, or the ticked apps. An app ticked here leaves whichever
    /// other channel had it.
    SetSource { ch: ChannelId, source: Source },
    /// A send's fader. On a linked channel the other send moves by the same amount, clamped.
    SetSend {
        ch: ChannelId,
        mix: MixId,
        position: f32,
    },
    /// A send's mute button.
    SetMute {
        ch: ChannelId,
        mix: MixId,
        mute: bool,
    },
    /// The link toggle between a channel's two faders.
    SetLink { ch: ChannelId, link: bool },
    /// MON or MST on a channel's FX header: whether `mix`'s send takes the processed signal.
    SetChainRouting { ch: ChannelId, mix: MixId, on: bool },
    /// The plugin browser: `plugin` into `strip`'s chain before `at` (`at == len` appends).
    AddInsert {
        strip: Strip,
        plugin: PluginRef,
        at: usize,
    },
    /// The trash icon on a slot.
    RemoveInsert { instance: InstanceId },
    /// A drag of an insert within or between strips; with `copy` (Ctrl) a second instance with
    /// the same plugin and state is made and the original stays.
    MoveInsert {
        from: SlotRef,
        to: SlotRef,
        copy: bool,
    },
    /// The bypass square.
    SetInsertEnabled { instance: InstanceId, enabled: bool },
    /// The plugin's state as `PluginState::encode` bytes, captured by the host before a save.
    SetInsertState {
        instance: InstanceId,
        state: Option<Vec<u8>>,
    },
    /// A mix's output radio. Monitor always has a device; Master may have none.
    SetMixDevice {
        mix: MixId,
        device: Option<DeviceRef>,
    },
    /// A mix's fader.
    SetMixLevel { mix: MixId, position: f32 },
    /// A mix's mute button.
    SetMixMute { mix: MixId, mute: bool },
    /// The ear button on Master.
    SetPreview(bool),
    /// The preferences dialog.
    SetPrefs(Prefs),
    /// The undo of a [`RemoveChannel`](Command::RemoveChannel): `channel` back at index `at`
    /// with its id, chain and instance ids, as it was removed. Refused when its id or an
    /// instance id is in use or was never handed out, past the channel limit, or past the
    /// end; an app another channel took since stays there.
    RestoreChannel { at: usize, channel: Channel },
    /// The undo of a [`RemoveInsert`](Command::RemoveInsert): `insert` back before `to` (the
    /// gap it was removed from) with its instance id, bypass and state. Refused when its
    /// instance id is in use or was never handed out, or past the chain's end.
    RestoreInsert { to: SlotRef, insert: Insert },
}

/// What a command changed, each part set exactly when its part of the setup changed. Every
/// field false and both lists empty means nothing changed and there is nothing to save.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[must_use = "Applied says what the host must do next: prepare, apply the graph, route, create, retire or save"]
pub struct Applied {
    /// The setup changed at all: the file on disk is stale, save it (debounced).
    pub save: bool,
    /// The engine's [`snapshot`](crate::snapshot()) changed: derive it again and prepare. A
    /// fader moved within silence changes its position (a save) but not its gain (no prepare).
    pub engine: bool,
    /// The backend's graph changed: the channels in order with their names and inputs (an
    /// app sink, or a capture on a device), a mix's device, or the quantum asked for.
    pub graph: bool,
    /// Which apps go to which channel changed: route them again.
    pub routing: bool,
    /// Instances the host must create (with their state, for a copy) before the engine lends,
    /// in chain order: inserts that entered the setup.
    pub created: Vec<InstanceId>,
    /// Instances the host must retire, in chain order: their inserts left the setup.
    pub removed: Vec<InstanceId>,
    /// The channel an `AddChannel` made or a `RestoreChannel` put back.
    pub channel: Option<ChannelId>,
}

impl Applied {
    /// Whether anything changed at all.
    pub fn changed(&self) -> bool {
        self.save
    }

    fn saved() -> Applied {
        Applied {
            save: true,
            ..Applied::default()
        }
    }

    /// A save when `changed`, else nothing.
    fn save_if(changed: bool) -> Applied {
        Applied {
            save: changed,
            ..Applied::default()
        }
    }

    /// A save and a prepare when `changed`, else nothing: for a change the snapshot carries
    /// as it is (a mute, a routing toggle, a bypass, the ear button).
    fn engine_if(changed: bool) -> Applied {
        Applied {
            save: changed,
            engine: changed,
            ..Applied::default()
        }
    }

    /// What moving faders from `before` to `after` changed: a save when a position or a mute
    /// moved, and a prepare too when what the engine applies (the linear gain) moved with it.
    fn faders(before: &[Send], after: &[Send]) -> Applied {
        Applied {
            save: before != after,
            engine: before
                .iter()
                .zip(after)
                .any(|(b, a)| snapshot::send(b) != snapshot::send(a)),
            ..Applied::default()
        }
    }
}

/// Why a command was refused. Each message says what to do instead.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Refused {
    #[error(
        "vix-mix mixes at most {max} channels and this setup has {max}: remove a channel before adding another"
    )]
    TooManyChannels { max: usize },
    #[error(
        "there is no channel {0:?} (it may have just been removed): read the setup again and send the command for a channel it lists"
    )]
    NoChannel(ChannelId),
    #[error(
        "there is no insert with instance {0:?} (it may have just been removed): read the setup again and send the command for an insert it lists"
    )]
    NoInstance(InstanceId),
    #[error(
        "{strip:?} has {len} inserts, so there is no slot {index}: name an insert below {len}, or a drop place up to {len}"
    )]
    NoSlot {
        strip: Strip,
        index: usize,
        len: usize,
    },
    #[error("a channel needs a name: type at least one character that is not a space")]
    EmptyName,
    #[error(
        "a fader position runs from 0 to 1 (unity is 0.84) and {0} is not one: clamp it to 0..=1 before sending"
    )]
    NotAPosition(f32),
    #[error(
        "Monitor always plays to a device: pick one of the outputs; only Master can have No device"
    )]
    MonitorNeedsDevice,
    #[error(
        "{id:?} is not a plugin id: a setup stores a plugin as `vst3:` and the 32 hexadecimal digits of its class id, as the catalog lists it"
    )]
    NotAPluginId { id: String },
    #[error(
        "{bundle:?} is a path: a setup stores a plugin's bundle by its file name (`Name.vst3`) and finds the path through the scan cache"
    )]
    BundleIsAPath { bundle: String },
    #[error("{quantum} is not a quantum the preferences offer: ask for 128, 256 or 512 frames")]
    NotAQuantum { quantum: u32 },
    #[error(
        "{max} plugin folders is the most there can be, not {found}: {}",
        remove_folders(.found, .max)
    )]
    TooManyPluginDirs { found: usize, max: usize },
    #[error(
        "this setup has handed out every {what} id there is: move setup.ron aside and start from a new setup"
    )]
    OutOfIds { what: &'static str },
    #[error(
        "the setup has {len} channels, so a channel cannot go back at place {at}: put it back at {len} or before"
    )]
    NoPlace { at: usize, len: usize },
    #[error(
        "{what} {id} is not one this setup removed (it is in use, or was never handed out): only a removed {what} can be put back; add a new one instead"
    )]
    NotRemoved { what: &'static str, id: u32 },
}

/// The fix [`Refused::TooManyPluginDirs`] teaches: one over the bound (an Add past it) in the
/// window's words, more than one by how many are left, because removing one is not enough.
fn remove_folders(found: &usize, max: &usize) -> String {
    if *found == *max + 1 {
        "remove one first".to_owned()
    } else {
        format!("remove folders until {max} are left")
    }
}

/// Applies `cmd` to `setup` whole, or refuses it and leaves `setup` as it was.
pub fn apply(setup: &mut Setup, cmd: Command) -> Result<Applied, Refused> {
    match cmd {
        Command::AddChannel { kind } => add_channel(setup, kind),
        Command::RemoveChannel(ch) => remove_channel(setup, ch),
        Command::Rename { ch, name } => rename(setup, ch, &name),
        Command::SetSource { ch, source } => set_source(setup, ch, source),
        Command::SetSend { ch, mix, position } => set_send(setup, ch, mix, position),
        Command::SetMute { ch, mix, mute } => {
            let send = channel_mut(setup, ch)?.send_mut(mix);
            Ok(Applied::engine_if(replace(&mut send.mute, mute) != mute))
        }
        Command::SetLink { ch, link } => {
            let channel = channel_mut(setup, ch)?;
            Ok(Applied::save_if(replace(&mut channel.link, link) != link))
        }
        Command::SetChainRouting { ch, mix, on } => {
            let channel = channel_mut(setup, ch)?;
            let flag = match mix {
                MixId::Monitor => &mut channel.chain_to_monitor,
                MixId::Master => &mut channel.chain_to_master,
            };
            Ok(Applied::engine_if(replace(flag, on) != on))
        }
        Command::AddInsert { strip, plugin, at } => add_insert(setup, strip, plugin, at),
        Command::RemoveInsert { instance } => {
            let (strip, index) = find_insert(setup, instance)?;
            chain_mut(setup, strip)?.remove(index);
            Ok(Applied {
                engine: true,
                removed: vec![instance],
                ..Applied::saved()
            })
        }
        Command::MoveInsert { from, to, copy } => move_insert(setup, from, to, copy),
        Command::SetInsertEnabled { instance, enabled } => {
            let insert = insert_mut(setup, instance)?;
            Ok(Applied::engine_if(
                replace(&mut insert.enabled, enabled) != enabled,
            ))
        }
        Command::SetInsertState { instance, state } => {
            let insert = insert_mut(setup, instance)?;
            // Saved, never processed: the live instance already holds this state.
            let changed = insert.state != state;
            insert.state = state;
            Ok(Applied::save_if(changed))
        }
        Command::SetMixDevice { mix, device } => {
            if mix == MixId::Monitor && device.is_none() {
                return Err(Refused::MonitorNeedsDevice);
            }
            let slot = &mut setup.mix_mut(mix).device;
            if *slot == device {
                return Ok(Applied::default());
            }
            *slot = device;
            Ok(Applied {
                graph: true,
                ..Applied::saved()
            })
        }
        Command::SetMixLevel { mix, position } => {
            let position = check_position(position)?;
            let level = &mut setup.mix_mut(mix).level;
            let before = *level;
            level.position = position;
            Ok(Applied::faders(&[before], &[*level]))
        }
        Command::SetMixMute { mix, mute } => {
            let level = &mut setup.mix_mut(mix).level;
            Ok(Applied::engine_if(replace(&mut level.mute, mute) != mute))
        }
        Command::SetPreview(on) => Ok(Applied::engine_if(
            replace(&mut setup.preview_master, on) != on,
        )),
        Command::SetPrefs(prefs) => {
            if !Prefs::QUANTA.contains(&prefs.quantum) {
                return Err(Refused::NotAQuantum {
                    quantum: prefs.quantum,
                });
            }
            if prefs.plugin_dirs.len() > Prefs::MAX_PLUGIN_DIRS {
                return Err(Refused::TooManyPluginDirs {
                    found: prefs.plugin_dirs.len(),
                    max: Prefs::MAX_PLUGIN_DIRS,
                });
            }
            if setup.prefs == prefs {
                return Ok(Applied::default());
            }
            let before = replace(&mut setup.prefs, prefs);
            Ok(Applied {
                // The quantum is asked of PipeWire through every node's `node.latency`.
                graph: before.quantum != setup.prefs.quantum,
                ..Applied::saved()
            })
        }
        Command::RestoreChannel { at, channel } => restore_channel(setup, at, channel),
        Command::RestoreInsert { to, insert } => restore_insert(setup, to, insert),
    }
}

fn add_channel(setup: &mut Setup, kind: ChannelKind) -> Result<Applied, Refused> {
    if setup.channels.len() >= MAX_CHANNELS {
        return Err(Refused::TooManyChannels { max: MAX_CHANNELS });
    }
    let id = ChannelId(take_id(&mut setup.next_channel.0, "channel")?);
    let (name, source) = match kind {
        ChannelKind::Hardware => ("Input", Source::Hardware(None)),
        ChannelKind::Apps => ("Apps", Source::Apps(Vec::new())),
    };
    setup.channels.push(Channel {
        id,
        name: name.to_string(),
        source,
        chain: Vec::new(),
        chain_to_monitor: true,
        chain_to_master: true,
        monitor: Send::UNITY,
        master: Send::UNITY,
        link: true,
    });
    Ok(Applied {
        engine: true,
        graph: true,
        channel: Some(id),
        ..Applied::saved()
    })
}

fn remove_channel(setup: &mut Setup, ch: ChannelId) -> Result<Applied, Refused> {
    let at = channel_index(setup, ch)?;
    let gone = setup.channels.remove(at);
    Ok(Applied {
        engine: true,
        graph: true,
        // Its apps go back to the default output.
        routing: !apps_of(&gone.source).is_empty(),
        removed: gone.chain.iter().map(|i| i.instance).collect(),
        ..Applied::saved()
    })
}

fn rename(setup: &mut Setup, ch: ChannelId, name: &str) -> Result<Applied, Refused> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Refused::EmptyName);
    }
    let channel = channel_mut(setup, ch)?;
    if channel.name == name {
        return Ok(Applied::default());
    }
    channel.name = name.to_string();
    // The channel's PipeWire node describes itself by this name.
    Ok(Applied {
        graph: true,
        ..Applied::saved()
    })
}

fn set_source(setup: &mut Setup, ch: ChannelId, source: Source) -> Result<Applied, Refused> {
    let at = channel_index(setup, ch)?;
    let source = match source {
        Source::Apps(apps) => {
            let mut unique: Vec<AppRef> = Vec::with_capacity(apps.len());
            for app in apps {
                if !unique.iter().any(|a| a.matches(&app)) {
                    unique.push(app);
                }
            }
            Source::Apps(unique)
        }
        hardware => hardware,
    };
    // Ticking an app here takes it out of every other channel: an app plays into one
    // channel only (the prototype's checklist does the same).
    let mut others_changed = false;
    if let Source::Apps(mine) = &source {
        for (index, other) in setup.channels.iter_mut().enumerate() {
            if index == at {
                continue;
            }
            if let Source::Apps(theirs) = &mut other.source {
                let before = theirs.len();
                theirs.retain(|a| !mine.iter().any(|m| m.matches(a)));
                others_changed |= theirs.len() != before;
            }
        }
    }
    let channel = &mut setup.channels[at];
    let before = replace(&mut channel.source, source);
    let after = &channel.source;
    if !others_changed && before == *after {
        return Ok(Applied::default());
    }
    let graph = match (&before, after) {
        (Source::Apps(_), Source::Apps(_)) => false,
        (Source::Hardware(a), Source::Hardware(b)) => a != b,
        _ => true,
    };
    let routing = others_changed || !same_apps(apps_of(&before), apps_of(after));
    Ok(Applied {
        graph,
        routing,
        ..Applied::saved()
    })
}

fn set_send(
    setup: &mut Setup,
    ch: ChannelId,
    mix: MixId,
    position: f32,
) -> Result<Applied, Refused> {
    let position = check_position(position)?;
    let channel = channel_mut(setup, ch)?;
    let before = [channel.monitor, channel.master];
    let was = channel.send(mix).position;
    channel.send_mut(mix).position = position;
    if channel.link {
        // The prototype's `setFader`: the partner moves by the same amount, clamped.
        let other = match mix {
            MixId::Monitor => MixId::Master,
            MixId::Master => MixId::Monitor,
        };
        let send = channel.send_mut(other);
        send.position = unsigned((send.position + (position - was)).clamp(0.0, 1.0));
    }
    Ok(Applied::faders(&before, &[channel.monitor, channel.master]))
}

fn add_insert(
    setup: &mut Setup,
    strip: Strip,
    plugin: PluginRef,
    at: usize,
) -> Result<Applied, Refused> {
    check_plugin(&plugin)?;
    let len = chain_mut(setup, strip)?.len();
    if at > len {
        return Err(Refused::NoSlot {
            strip,
            index: at,
            len,
        });
    }
    let instance = InstanceId(take_id(&mut setup.next_instance.0, "plugin instance")?);
    chain_mut(setup, strip)?.insert(
        at,
        Insert {
            instance,
            plugin,
            enabled: true,
            state: None,
        },
    );
    Ok(Applied {
        engine: true,
        created: vec![instance],
        ..Applied::saved()
    })
}

fn move_insert(
    setup: &mut Setup,
    from: SlotRef,
    to: SlotRef,
    copy: bool,
) -> Result<Applied, Refused> {
    let from_len = chain_mut(setup, from.strip)?.len();
    if from.index >= from_len {
        return Err(Refused::NoSlot {
            strip: from.strip,
            index: from.index,
            len: from_len,
        });
    }
    let to_len = chain_mut(setup, to.strip)?.len();
    if to.index > to_len {
        return Err(Refused::NoSlot {
            strip: to.strip,
            index: to.index,
            len: to_len,
        });
    }
    let same = from.strip == to.strip;
    if copy {
        // A second instance with the same plugin, bypass and state; the original stays.
        let mut item = chain_mut(setup, from.strip)?[from.index].clone();
        item.instance = InstanceId(take_id(&mut setup.next_instance.0, "plugin instance")?);
        let created = item.instance;
        chain_mut(setup, to.strip)?.insert(to.index, item);
        return Ok(Applied {
            engine: true,
            created: vec![created],
            ..Applied::saved()
        });
    }
    // Dropped on its own place, or on the gap just below it: nothing moves.
    if same && (to.index == from.index || to.index == from.index + 1) {
        return Ok(Applied::default());
    }
    let item = chain_mut(setup, from.strip)?.remove(from.index);
    let at = if same && to.index > from.index {
        to.index - 1
    } else {
        to.index
    };
    // The same instance moves: no instance is created or retired.
    chain_mut(setup, to.strip)?.insert(at, item);
    Ok(Applied {
        engine: true,
        ..Applied::saved()
    })
}

fn restore_channel(setup: &mut Setup, at: usize, channel: Channel) -> Result<Applied, Refused> {
    let len = setup.channels.len();
    if len >= MAX_CHANNELS {
        return Err(Refused::TooManyChannels { max: MAX_CHANNELS });
    }
    if at > len {
        return Err(Refused::NoPlace { at, len });
    }
    let id = channel.id;
    if setup.channel(id).is_some() || id.0 >= setup.next_channel.0 {
        return Err(Refused::NotRemoved {
            what: "channel",
            id: id.0,
        });
    }
    let name = channel.name.trim();
    if name.is_empty() {
        return Err(Refused::EmptyName);
    }
    let monitor = check_send(channel.monitor)?;
    let master = check_send(channel.master)?;
    for (index, insert) in channel.chain.iter().enumerate() {
        check_removed_insert(setup, insert)?;
        if channel.chain[..index]
            .iter()
            .any(|i| i.instance == insert.instance)
        {
            return Err(Refused::NotRemoved {
                what: "plugin instance",
                id: insert.instance.0,
            });
        }
    }
    // An app plays into one channel only: one another channel took since stays there.
    let source = match channel.source {
        Source::Apps(apps) => {
            let mut kept: Vec<AppRef> = Vec::with_capacity(apps.len());
            for app in apps {
                if setup.channel_of_app(&app).is_none() && !kept.iter().any(|a| a.matches(&app)) {
                    kept.push(app);
                }
            }
            Source::Apps(kept)
        }
        hardware => hardware,
    };
    let restored = Channel {
        id,
        name: name.to_string(),
        source,
        monitor,
        master,
        ..channel
    };
    let routing = !apps_of(&restored.source).is_empty();
    let created = restored.chain.iter().map(|i| i.instance).collect();
    setup.channels.insert(at, restored);
    Ok(Applied {
        engine: true,
        graph: true,
        routing,
        created,
        channel: Some(id),
        ..Applied::saved()
    })
}

fn restore_insert(setup: &mut Setup, to: SlotRef, insert: Insert) -> Result<Applied, Refused> {
    let len = chain_mut(setup, to.strip)?.len();
    if to.index > len {
        return Err(Refused::NoSlot {
            strip: to.strip,
            index: to.index,
            len,
        });
    }
    check_removed_insert(setup, &insert)?;
    let instance = insert.instance;
    chain_mut(setup, to.strip)?.insert(to.index, insert);
    Ok(Applied {
        engine: true,
        created: vec![instance],
        ..Applied::saved()
    })
}

/// Whether `insert` can be put back: its plugin stored as a setup stores one, and its
/// instance an id this setup handed out and no insert holds now.
fn check_removed_insert(setup: &Setup, insert: &Insert) -> Result<(), Refused> {
    check_plugin(&insert.plugin)?;
    let id = insert.instance;
    if setup.inserts().any(|i| i.instance == id) || id.0 >= setup.next_instance.0 {
        return Err(Refused::NotRemoved {
            what: "plugin instance",
            id: id.0,
        });
    }
    Ok(())
}

/// `send` with a fader position, -0 read as 0.
fn check_send(send: Send) -> Result<Send, Refused> {
    Ok(Send {
        position: check_position(send.position)?,
        ..send
    })
}

/// Hands out `*next` and moves it on, or refuses when the id space is spent. `u32::MAX` is
/// never handed out, so a counter that reached it stays there and refuses.
fn take_id(next: &mut u32, what: &'static str) -> Result<u32, Refused> {
    let id = *next;
    *next = id.checked_add(1).ok_or(Refused::OutOfIds { what })?;
    Ok(id)
}

/// `position` when it is a fader position, with -0 read as 0 so a setup never stores both.
fn check_position(position: f32) -> Result<f32, Refused> {
    if (0.0..=1.0).contains(&position) {
        Ok(unsigned(position))
    } else {
        Err(Refused::NotAPosition(position))
    }
}

/// -0 as 0; every other value as it is.
fn unsigned(position: f32) -> f32 {
    position + 0.0
}

/// Whether `plugin` is stored as a setup stores a plugin: `vst3:` and 32 hexadecimal digits,
/// and the bundle by its file name.
pub(crate) fn check_plugin(plugin: &PluginRef) -> Result<(), Refused> {
    let hex = plugin.id.strip_prefix("vst3:").unwrap_or("");
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Refused::NotAPluginId {
            id: plugin.id.clone(),
        });
    }
    if plugin.bundle.contains(['/', '\\']) {
        return Err(Refused::BundleIsAPath {
            bundle: plugin.bundle.clone(),
        });
    }
    Ok(())
}

fn apps_of(source: &Source) -> &[AppRef] {
    match source {
        Source::Apps(apps) => apps,
        Source::Hardware(_) => &[],
    }
}

/// Whether two app lists hold the same apps in any order. Neither lists an app twice.
fn same_apps(a: &[AppRef], b: &[AppRef]) -> bool {
    a.len() == b.len() && a.iter().all(|app| b.contains(app))
}

fn channel_index(setup: &Setup, ch: ChannelId) -> Result<usize, Refused> {
    setup
        .channels
        .iter()
        .position(|c| c.id == ch)
        .ok_or(Refused::NoChannel(ch))
}

fn channel_mut(setup: &mut Setup, ch: ChannelId) -> Result<&mut Channel, Refused> {
    setup.channel_mut(ch).ok_or(Refused::NoChannel(ch))
}

fn chain_mut(setup: &mut Setup, strip: Strip) -> Result<&mut Vec<Insert>, Refused> {
    match strip {
        Strip::Channel(ch) => Ok(&mut channel_mut(setup, ch)?.chain),
        Strip::Mix(mix) => Ok(&mut setup.mix_mut(mix).chain),
    }
}

fn find_insert(setup: &Setup, instance: InstanceId) -> Result<(Strip, usize), Refused> {
    let strips = setup
        .channels
        .iter()
        .map(|c| (Strip::Channel(c.id), &c.chain))
        .chain([
            (Strip::Mix(MixId::Monitor), &setup.monitor.chain),
            (Strip::Mix(MixId::Master), &setup.master.chain),
        ]);
    for (strip, chain) in strips {
        if let Some(index) = chain.iter().position(|i| i.instance == instance) {
            return Ok((strip, index));
        }
    }
    Err(Refused::NoInstance(instance))
}

fn insert_mut(setup: &mut Setup, instance: InstanceId) -> Result<&mut Insert, Refused> {
    let (strip, index) = find_insert(setup, instance)?;
    Ok(&mut chain_mut(setup, strip)?[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taper::UNITY;

    fn app(binary: &str) -> AppRef {
        AppRef {
            app_id: None,
            binary: Some(binary.to_string()),
            name: binary.to_string(),
        }
    }

    fn plugin(name: &str) -> PluginRef {
        PluginRef {
            id: format!("vst3:{:032X}", name.len()),
            bundle: format!("{name}.vst3"),
            name: name.to_string(),
        }
    }

    fn device(name: &str) -> DeviceRef {
        DeviceRef {
            name: name.to_string(),
            description: name.to_string(),
        }
    }

    fn names(chain: &[Insert]) -> Vec<&str> {
        chain.iter().map(|i| i.plugin.name.as_str()).collect()
    }

    fn add(setup: &mut Setup, kind: ChannelKind) -> ChannelId {
        apply(setup, Command::AddChannel { kind })
            .unwrap()
            .channel
            .unwrap()
    }

    #[test]
    fn a_ninth_channel_is_refused_with_the_limit() {
        let mut setup = Setup::default();
        for _ in 1..MAX_CHANNELS {
            let applied = apply(
                &mut setup,
                Command::AddChannel {
                    kind: ChannelKind::Apps,
                },
            )
            .unwrap();
            assert!(applied.graph && applied.engine && applied.channel.is_some());
        }
        let before = setup.clone();
        let refused = apply(
            &mut setup,
            Command::AddChannel {
                kind: ChannelKind::Hardware,
            },
        )
        .unwrap_err();
        assert_eq!(refused, Refused::TooManyChannels { max: 8 });
        let message = refused.to_string();
        assert!(message.contains("at most 8 channels"), "{message}");
        assert!(message.contains("remove a channel"), "{message}");
        assert_eq!(setup, before, "a refused command changes nothing");
    }

    #[test]
    fn an_app_ticked_in_channel_b_leaves_channel_a() {
        let mut setup = Setup::default();
        let a = add(&mut setup, ChannelKind::Apps);
        let b = add(&mut setup, ChannelKind::Apps);
        let ticked = apply(
            &mut setup,
            Command::SetSource {
                ch: a,
                source: Source::Apps(vec![app("spotify"), app("firefox")]),
            },
        )
        .unwrap();
        assert!(ticked.routing && !ticked.graph && !ticked.engine);
        let applied = apply(
            &mut setup,
            Command::SetSource {
                ch: b,
                source: Source::Apps(vec![app("spotify"), app("spotify")]),
            },
        )
        .unwrap();
        assert!(applied.routing && applied.save);
        assert!(!applied.graph, "an app sink stays an app sink");
        assert_eq!(
            setup.channel(a).unwrap().source,
            Source::Apps(vec![app("firefox")])
        );
        assert_eq!(
            setup.channel(b).unwrap().source,
            Source::Apps(vec![app("spotify")]),
            "ticked twice, listed once"
        );
        assert_eq!(setup.channel_of_app(&app("spotify")), Some(b));
        // The same apps in another order route nothing anew; the file still changes.
        let _ = apply(
            &mut setup,
            Command::SetSource {
                ch: a,
                source: Source::Apps(vec![app("firefox"), app("mpv")]),
            },
        )
        .unwrap();
        let reordered = apply(
            &mut setup,
            Command::SetSource {
                ch: a,
                source: Source::Apps(vec![app("mpv"), app("firefox")]),
            },
        )
        .unwrap();
        assert!(reordered.save && !reordered.routing);
    }

    #[test]
    fn unknown_ids_are_refused_and_change_nothing() {
        let mut setup = Setup::default();
        let before = setup.clone();
        let missing = ChannelId(99);
        for cmd in [
            Command::RemoveChannel(missing),
            Command::SetMute {
                ch: missing,
                mix: MixId::Master,
                mute: true,
            },
            Command::SetSource {
                ch: missing,
                source: Source::Apps(vec![app("firefox")]),
            },
            Command::AddInsert {
                strip: Strip::Channel(missing),
                plugin: plugin("EQ"),
                at: 0,
            },
        ] {
            assert_eq!(apply(&mut setup, cmd), Err(Refused::NoChannel(missing)));
        }
        assert_eq!(
            apply(
                &mut setup,
                Command::RemoveInsert {
                    instance: InstanceId(7)
                }
            ),
            Err(Refused::NoInstance(InstanceId(7)))
        );
        let refused = apply(
            &mut setup,
            Command::AddInsert {
                strip: Strip::Mix(MixId::Master),
                plugin: plugin("EQ"),
                at: 1,
            },
        )
        .unwrap_err();
        assert!(matches!(
            refused,
            Refused::NoSlot {
                index: 1,
                len: 0,
                ..
            }
        ));
        assert_eq!(setup, before, "not even an id was spent");
    }

    #[test]
    fn a_removed_id_never_comes_back() {
        let mut setup = Setup::default();
        let strip = Strip::Channel(setup.channels[0].id);
        let first = apply(
            &mut setup,
            Command::AddInsert {
                strip,
                plugin: plugin("EQ"),
                at: 0,
            },
        )
        .unwrap()
        .created[0];
        let removed = apply(&mut setup, Command::RemoveInsert { instance: first }).unwrap();
        assert_eq!(removed.removed, [first]);
        // The host still holds `first` while the undo toast shows: the next insert must not
        // take its id, although no insert in the setup uses it any more.
        let second = apply(
            &mut setup,
            Command::AddInsert {
                strip,
                plugin: plugin("EQ"),
                at: 0,
            },
        )
        .unwrap()
        .created[0];
        assert_ne!(second, first);
        let copy = apply(
            &mut setup,
            Command::MoveInsert {
                from: SlotRef { strip, index: 0 },
                to: SlotRef { strip, index: 1 },
                copy: true,
            },
        )
        .unwrap()
        .created[0];
        assert!(copy != first && copy != second);

        let mic = setup.channels[0].id;
        let input = add(&mut setup, ChannelKind::Hardware);
        let _ = apply(&mut setup, Command::RemoveChannel(input)).unwrap();
        let again = add(&mut setup, ChannelKind::Hardware);
        assert!(again != input && again != mic);
    }

    #[test]
    fn a_spent_id_space_is_refused_and_changes_nothing() {
        let mut setup = Setup {
            next_instance: InstanceId(u32::MAX),
            next_channel: ChannelId(u32::MAX),
            ..Setup::default()
        };
        let before = setup.clone();
        let refused = apply(
            &mut setup,
            Command::AddInsert {
                strip: Strip::Mix(MixId::Monitor),
                plugin: plugin("EQ"),
                at: 0,
            },
        )
        .unwrap_err();
        assert!(refused.to_string().contains("start from a new setup"));
        assert!(matches!(
            apply(
                &mut setup,
                Command::AddChannel {
                    kind: ChannelKind::Apps
                }
            ),
            Err(Refused::OutOfIds { .. })
        ));
        assert_eq!(setup, before);
    }

    #[test]
    fn a_command_that_changes_nothing_says_so() {
        let mut setup = Setup::default();
        let ch = setup.channels[0].id;
        let strip = Strip::Channel(ch);
        let instance = apply(
            &mut setup,
            Command::AddInsert {
                strip,
                plugin: plugin("EQ"),
                at: 0,
            },
        )
        .unwrap()
        .created[0];
        let _ = apply(
            &mut setup,
            Command::SetMixDevice {
                mix: MixId::Monitor,
                device: Some(device("phones")),
            },
        )
        .unwrap();
        let before = setup.clone();
        for cmd in [
            Command::Rename {
                ch,
                name: " Mic ".to_string(),
            },
            Command::SetSource {
                ch,
                source: Source::Hardware(None),
            },
            Command::SetSend {
                ch,
                mix: MixId::Monitor,
                position: UNITY,
            },
            Command::SetMute {
                ch,
                mix: MixId::Master,
                mute: false,
            },
            Command::SetLink { ch, link: true },
            Command::SetChainRouting {
                ch,
                mix: MixId::Monitor,
                on: true,
            },
            Command::MoveInsert {
                from: SlotRef { strip, index: 0 },
                to: SlotRef { strip, index: 1 },
                copy: false,
            },
            Command::SetInsertEnabled {
                instance,
                enabled: true,
            },
            Command::SetInsertState {
                instance,
                state: None,
            },
            Command::SetMixDevice {
                mix: MixId::Monitor,
                device: Some(device("phones")),
            },
            Command::SetMixLevel {
                mix: MixId::Master,
                position: UNITY,
            },
            Command::SetMixMute {
                mix: MixId::Master,
                mute: false,
            },
            Command::SetPreview(false),
            Command::SetPrefs(Prefs::default()),
        ] {
            let applied = apply(&mut setup, cmd.clone()).unwrap();
            assert_eq!(applied, Applied::default(), "{cmd:?}");
            assert!(!applied.changed());
        }
        assert_eq!(setup, before);
    }

    #[test]
    fn a_fader_moved_within_silence_is_saved_but_not_prepared() {
        let mut setup = Setup::default();
        let ch = setup.channels[0].id;
        let _ = apply(&mut setup, Command::SetLink { ch, link: false }).unwrap();
        let down = apply(
            &mut setup,
            Command::SetSend {
                ch,
                mix: MixId::Master,
                position: 0.0,
            },
        )
        .unwrap();
        assert!(down.save && down.engine);
        // 0.001 is still at or below 0.002: silent, so the gain the engine applies is 0.
        let nudged = apply(
            &mut setup,
            Command::SetSend {
                ch,
                mix: MixId::Master,
                position: 0.001,
            },
        )
        .unwrap();
        assert!(nudged.save && !nudged.engine, "{nudged:?}");
        // A muted send's level still reaches the engine: unmuting must play it at that gain.
        let _ = apply(
            &mut setup,
            Command::SetMute {
                ch,
                mix: MixId::Monitor,
                mute: true,
            },
        )
        .unwrap();
        let muted_move = apply(
            &mut setup,
            Command::SetSend {
                ch,
                mix: MixId::Monitor,
                position: 0.5,
            },
        )
        .unwrap();
        assert!(muted_move.save && muted_move.engine);
    }

    #[test]
    fn a_linked_fader_moves_its_partner_by_the_same_amount_clamped() {
        let mut setup = Setup::default();
        let ch = setup.channels[0].id;
        let _ = apply(
            &mut setup,
            Command::SetSend {
                ch,
                mix: MixId::Monitor,
                position: UNITY + 0.1,
            },
        )
        .unwrap();
        let channel = setup.channel(ch).unwrap();
        assert!((channel.master.position - (UNITY + 0.1)).abs() < 1e-6);
        let _ = apply(
            &mut setup,
            Command::SetSend {
                ch,
                mix: MixId::Master,
                position: 1.0,
            },
        )
        .unwrap();
        let channel = setup.channel(ch).unwrap();
        assert!((channel.monitor.position - 1.0).abs() < 1e-6);
        // Down to the bottom: both clamp at 0, never below.
        let _ = apply(
            &mut setup,
            Command::SetSend {
                ch,
                mix: MixId::Master,
                position: 0.0,
            },
        )
        .unwrap();
        let channel = setup.channel(ch).unwrap();
        assert_eq!(channel.monitor.position, 0.0);
        assert!(channel.monitor.position.is_sign_positive());
        for position in [f32::NAN, -0.1, 1.5] {
            assert!(matches!(
                apply(
                    &mut setup,
                    Command::SetSend {
                        ch,
                        mix: MixId::Master,
                        position,
                    }
                ),
                Err(Refused::NotAPosition(_))
            ));
        }
        // An unlinked channel moves one fader only.
        let _ = apply(&mut setup, Command::SetLink { ch, link: false }).unwrap();
        let _ = apply(
            &mut setup,
            Command::SetSend {
                ch,
                mix: MixId::Monitor,
                position: 0.5,
            },
        )
        .unwrap();
        assert_eq!(setup.channel(ch).unwrap().master.position, 0.0);
    }

    #[test]
    fn inserts_move_within_and_between_strips_as_the_prototype_drops_them() {
        let mut setup = Setup::default();
        let ch = Strip::Channel(setup.channels[0].id);
        let master = Strip::Mix(MixId::Master);
        for (at, name) in ["A", "B", "C"].into_iter().enumerate() {
            let applied = apply(
                &mut setup,
                Command::AddInsert {
                    strip: ch,
                    plugin: plugin(name),
                    at,
                },
            )
            .unwrap();
            assert_eq!(applied.created.len(), 1);
        }
        let slot = |strip, index| SlotRef { strip, index };
        // On its own place or the gap below it: nothing changes, nothing to save.
        for to in [0, 1] {
            let applied = apply(
                &mut setup,
                Command::MoveInsert {
                    from: slot(ch, 0),
                    to: slot(ch, to),
                    copy: false,
                },
            )
            .unwrap();
            assert!(!applied.changed());
        }
        // A to below C.
        let _ = apply(
            &mut setup,
            Command::MoveInsert {
                from: slot(ch, 0),
                to: slot(ch, 3),
                copy: false,
            },
        )
        .unwrap();
        assert_eq!(names(&setup.channels[0].chain), ["B", "C", "A"]);
        // A back above B: an upward move takes the index as it is.
        let _ = apply(
            &mut setup,
            Command::MoveInsert {
                from: slot(ch, 2),
                to: slot(ch, 0),
                copy: false,
            },
        )
        .unwrap();
        assert_eq!(names(&setup.channels[0].chain), ["A", "B", "C"]);
        let _ = apply(
            &mut setup,
            Command::MoveInsert {
                from: slot(ch, 0),
                to: slot(ch, 3),
                copy: false,
            },
        )
        .unwrap();
        assert_eq!(names(&setup.channels[0].chain), ["B", "C", "A"]);
        // C onto Master keeps its instance; a copy gets a new one and keeps the state.
        let c = setup.channels[0].chain[1].instance;
        let _ = apply(
            &mut setup,
            Command::SetInsertState {
                instance: c,
                state: Some(vec![1, 2, 3]),
            },
        )
        .unwrap();
        let _ = apply(
            &mut setup,
            Command::SetInsertEnabled {
                instance: c,
                enabled: false,
            },
        )
        .unwrap();
        let moved = apply(
            &mut setup,
            Command::MoveInsert {
                from: slot(ch, 1),
                to: slot(master, 0),
                copy: false,
            },
        )
        .unwrap();
        assert!(moved.created.is_empty() && moved.removed.is_empty() && moved.engine);
        assert_eq!(setup.master.chain[0].instance, c);
        let copied = apply(
            &mut setup,
            Command::MoveInsert {
                from: slot(master, 0),
                to: slot(ch, 0),
                copy: true,
            },
        )
        .unwrap();
        assert_eq!(copied.created.len(), 1);
        assert_ne!(copied.created[0], c);
        let copy = &setup.channels[0].chain[0];
        assert_eq!(copy.state, Some(vec![1, 2, 3]));
        assert!(!copy.enabled, "a copy keeps the bypass");
        assert_eq!(names(&setup.channels[0].chain), ["C", "B", "A"]);
        assert_eq!(names(&setup.master.chain), ["C"]);
    }

    #[test]
    fn removing_a_channel_retires_its_instances_and_returns_its_apps() {
        let mut setup = Setup::default();
        let ch = add(&mut setup, ChannelKind::Apps);
        let mut made = Vec::new();
        for name in ["A", "B"] {
            made.extend(
                apply(
                    &mut setup,
                    Command::AddInsert {
                        strip: Strip::Channel(ch),
                        plugin: plugin(name),
                        at: made.len(),
                    },
                )
                .unwrap()
                .created,
            );
        }
        let _ = apply(
            &mut setup,
            Command::SetSource {
                ch,
                source: Source::Apps(vec![app("spotify")]),
            },
        )
        .unwrap();
        let applied = apply(&mut setup, Command::RemoveChannel(ch)).unwrap();
        assert_eq!(applied.removed, made);
        assert!(applied.routing && applied.graph && applied.engine);
        // An empty app channel routed nothing, so removing it routes nothing.
        let empty = add(&mut setup, ChannelKind::Apps);
        let applied = apply(&mut setup, Command::RemoveChannel(empty)).unwrap();
        assert!(!applied.routing && applied.removed.is_empty());
    }

    #[test]
    fn a_plugin_is_stored_by_id_and_bundle_name_never_by_path() {
        let mut setup = Setup::default();
        let strip = Strip::Mix(MixId::Monitor);
        let mut bad = plugin("EQ");
        bad.id = "vst3:nothex".to_string();
        assert!(matches!(
            apply(
                &mut setup,
                Command::AddInsert {
                    strip,
                    plugin: bad,
                    at: 0
                }
            ),
            Err(Refused::NotAPluginId { .. })
        ));
        let mut path = plugin("EQ");
        path.bundle = "/usr/lib/vst3/EQ.vst3".to_string();
        let refused = apply(
            &mut setup,
            Command::AddInsert {
                strip,
                plugin: path,
                at: 0,
            },
        )
        .unwrap_err();
        assert!(refused.to_string().contains("file name"));
        let mut lower = plugin("EQ");
        lower.id = lower.id.to_lowercase();
        assert!(
            apply(
                &mut setup,
                Command::AddInsert {
                    strip,
                    plugin: lower,
                    at: 0,
                }
            )
            .is_ok(),
            "hex digits in either case"
        );
    }

    #[test]
    fn monitor_always_has_a_device_and_master_may_have_none() {
        let mut setup = Setup::default();
        assert_eq!(
            apply(
                &mut setup,
                Command::SetMixDevice {
                    mix: MixId::Monitor,
                    device: None
                }
            ),
            Err(Refused::MonitorNeedsDevice)
        );
        let applied = apply(
            &mut setup,
            Command::SetMixDevice {
                mix: MixId::Master,
                device: Some(device("out")),
            },
        )
        .unwrap();
        assert!(applied.graph && !applied.engine);
        let applied = apply(
            &mut setup,
            Command::SetMixDevice {
                mix: MixId::Master,
                device: None,
            },
        )
        .unwrap();
        assert!(applied.graph);
        assert_eq!(setup.master.device, None);
    }

    #[test]
    fn the_quantum_is_one_the_preferences_offer_and_changing_it_changes_the_graph() {
        let mut setup = Setup::default();
        let refused = apply(
            &mut setup,
            Command::SetPrefs(Prefs {
                quantum: 300,
                ..Prefs::default()
            }),
        )
        .unwrap_err();
        assert!(refused.to_string().contains("128, 256 or 512"));
        let applied = apply(
            &mut setup,
            Command::SetPrefs(Prefs {
                quantum: 128,
                ..Prefs::default()
            }),
        )
        .unwrap();
        assert!(applied.graph && applied.save && !applied.engine);
        let applied = apply(
            &mut setup,
            Command::SetPrefs(Prefs {
                quantum: 128,
                plugin_dirs: vec!["/opt/vst3".into()],
            }),
        )
        .unwrap();
        assert!(applied.save && !applied.graph);
    }

    #[test]
    fn a_seventeenth_plugin_folder_is_refused_with_the_limit_and_the_fix() {
        let folders = |n: usize| -> Vec<std::path::PathBuf> {
            (0..n).map(|i| format!("/opt/vst3/{i}").into()).collect()
        };
        let mut setup = Setup::default();
        // Sixteen, the most there can be, are taken.
        let full = Prefs {
            plugin_dirs: folders(Prefs::MAX_PLUGIN_DIRS),
            ..Prefs::default()
        };
        let applied = apply(&mut setup, Command::SetPrefs(full.clone())).unwrap();
        assert!(applied.save && !applied.graph);
        assert_eq!(setup.prefs, full);
        // A seventeenth is refused, whatever else the command asks, and nothing changes.
        let before = setup.clone();
        let refused = apply(
            &mut setup,
            Command::SetPrefs(Prefs {
                quantum: 512,
                plugin_dirs: folders(Prefs::MAX_PLUGIN_DIRS + 1),
            }),
        )
        .unwrap_err();
        assert_eq!(refused, Refused::TooManyPluginDirs { found: 17, max: 16 });
        let message = refused.to_string();
        assert!(
            message.starts_with("16 plugin folders is the most there can be"),
            "{message}"
        );
        assert!(message.ends_with("remove one first"), "{message}");
        assert_eq!(setup, before);
        // Further past the bound, removing one is not enough: it says how many are left.
        let refused = apply(
            &mut setup,
            Command::SetPrefs(Prefs {
                plugin_dirs: folders(20),
                ..Prefs::default()
            }),
        )
        .unwrap_err();
        assert_eq!(
            refused.to_string(),
            "16 plugin folders is the most there can be, not 20: remove folders until 16 are left"
        );
        assert_eq!(setup, before);
    }

    #[test]
    fn a_removed_channel_comes_back_in_its_place_as_it_was() {
        let mut setup = Setup::default();
        let ch = add(&mut setup, ChannelKind::Apps);
        let after = add(&mut setup, ChannelKind::Hardware);
        let made = apply(
            &mut setup,
            Command::AddInsert {
                strip: Strip::Channel(ch),
                plugin: plugin("EQ"),
                at: 0,
            },
        )
        .unwrap()
        .created;
        let instance = made[0];
        for cmd in [
            Command::SetSource {
                ch,
                source: Source::Apps(vec![app("spotify"), app("firefox")]),
            },
            Command::SetInsertEnabled {
                instance,
                enabled: false,
            },
            Command::SetInsertState {
                instance,
                state: Some(vec![1, 2, 3]),
            },
        ] {
            let _ = apply(&mut setup, cmd).unwrap();
        }
        let at = setup.channels.iter().position(|c| c.id == ch).unwrap();
        let gone = setup.channels[at].clone();
        let _ = apply(&mut setup, Command::RemoveChannel(ch)).unwrap();
        // Firefox goes to another channel while the toast shows; it stays there.
        let _ = apply(
            &mut setup,
            Command::SetSource {
                ch: after,
                source: Source::Apps(vec![app("firefox")]),
            },
        )
        .unwrap();
        let applied = apply(
            &mut setup,
            Command::RestoreChannel {
                at,
                channel: gone.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            applied.created, made,
            "its instances come back under their ids"
        );
        assert_eq!(applied.channel, Some(ch));
        assert!(applied.engine && applied.graph && applied.routing && applied.save);
        let back = &setup.channels[at];
        assert_eq!(back.id, ch, "in its place, under its id");
        assert_eq!(back.chain, gone.chain, "bypass and state as they were");
        assert_eq!(back.source, Source::Apps(vec![app("spotify")]));
        assert_eq!(setup.channel_of_app(&app("firefox")), Some(after));
        assert_eq!(setup.next_channel, ChannelId(after.0 + 1), "no id is spent");
        // Put back once; a second time it is in use.
        let again = apply(&mut setup, Command::RestoreChannel { at, channel: gone });
        assert_eq!(
            again,
            Err(Refused::NotRemoved {
                what: "channel",
                id: ch.0
            })
        );
    }

    #[test]
    fn a_removed_insert_comes_back_in_its_gap_as_it_was() {
        let mut setup = Setup::default();
        let ch = setup.channels[0].id;
        let strip = Strip::Channel(ch);
        let mut made = Vec::new();
        for (at, name) in ["A", "B", "C"].into_iter().enumerate() {
            made.extend(
                apply(
                    &mut setup,
                    Command::AddInsert {
                        strip,
                        plugin: plugin(name),
                        at,
                    },
                )
                .unwrap()
                .created,
            );
        }
        let _ = apply(
            &mut setup,
            Command::SetInsertState {
                instance: made[1],
                state: Some(vec![7]),
            },
        )
        .unwrap();
        let gone = setup.channels[0].chain[1].clone();
        let _ = apply(&mut setup, Command::RemoveInsert { instance: made[1] }).unwrap();
        let applied = apply(
            &mut setup,
            Command::RestoreInsert {
                to: SlotRef { strip, index: 1 },
                insert: gone.clone(),
            },
        )
        .unwrap();
        assert_eq!(applied.created, vec![made[1]]);
        assert!(applied.engine && applied.save && !applied.graph && !applied.routing);
        assert_eq!(names(&setup.channels[0].chain), ["A", "B", "C"]);
        assert_eq!(setup.channels[0].chain[1], gone);
    }

    #[test]
    fn only_what_was_removed_can_be_put_back() {
        let mut setup = Setup::default();
        let ch = setup.channels[0].id;
        let strip = Strip::Channel(ch);
        let made = apply(
            &mut setup,
            Command::AddInsert {
                strip,
                plugin: plugin("EQ"),
                at: 0,
            },
        )
        .unwrap()
        .created;
        let live = setup.channels[0].chain[0].clone();
        let mut never = live.clone();
        never.instance = setup.next_instance;
        let channel = setup.channels[0].clone();
        let mut unknown = channel.clone();
        unknown.id = setup.next_channel;
        unknown.chain.clear();
        let before = setup.clone();
        for (cmd, refused) in [
            (
                Command::RestoreInsert {
                    to: SlotRef { strip, index: 0 },
                    insert: live,
                },
                Refused::NotRemoved {
                    what: "plugin instance",
                    id: made[0].0,
                },
            ),
            (
                Command::RestoreInsert {
                    to: SlotRef { strip, index: 0 },
                    insert: never.clone(),
                },
                Refused::NotRemoved {
                    what: "plugin instance",
                    id: never.instance.0,
                },
            ),
            (
                Command::RestoreInsert {
                    to: SlotRef { strip, index: 2 },
                    insert: never,
                },
                Refused::NoSlot {
                    strip,
                    index: 2,
                    len: 1,
                },
            ),
            (
                Command::RestoreChannel {
                    at: 0,
                    channel: channel.clone(),
                },
                Refused::NotRemoved {
                    what: "channel",
                    id: ch.0,
                },
            ),
            (
                Command::RestoreChannel {
                    at: 0,
                    channel: unknown.clone(),
                },
                Refused::NotRemoved {
                    what: "channel",
                    id: unknown.id.0,
                },
            ),
        ] {
            let message = refused.to_string();
            assert!(message.contains(": "), "{message} says no fix");
            assert_eq!(apply(&mut setup, cmd), Err(refused));
            assert_eq!(setup, before, "a refused restore changes nothing");
        }
        let _ = apply(&mut setup, Command::RemoveChannel(ch)).unwrap();
        let place = apply(
            &mut setup,
            Command::RestoreChannel {
                at: 1,
                channel: channel.clone(),
            },
        );
        assert_eq!(place, Err(Refused::NoPlace { at: 1, len: 0 }));
        let mut nameless = channel.clone();
        nameless.name = "  ".to_string();
        let name = apply(
            &mut setup,
            Command::RestoreChannel {
                at: 0,
                channel: nameless,
            },
        );
        assert_eq!(name, Err(Refused::EmptyName));
        for _ in 0..MAX_CHANNELS {
            add(&mut setup, ChannelKind::Apps);
        }
        let full = apply(&mut setup, Command::RestoreChannel { at: 0, channel });
        assert_eq!(full, Err(Refused::TooManyChannels { max: MAX_CHANNELS }));
    }
}
