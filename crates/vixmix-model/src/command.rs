//! Every change to a [`Setup`] is a [`Command`] passed to [`apply`], which validates it,
//! applies it whole or not at all, and says in [`Applied`] what it changed, so the host knows
//! whether to prepare the engine again, apply the backend's graph, route apps, create or retire
//! plugin instances, or only save.

use crate::ids::{ChannelId, InstanceId, MixId};
use crate::setup::{
    AppRef, Channel, ChannelKind, DeviceRef, Insert, MAX_CHANNELS, PluginRef, Prefs, Send, Setup,
    Source,
};

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
}

/// What a command changed. Every field false and both lists empty means nothing changed (a
/// drag dropped where it started) and there is nothing to save.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[must_use = "Applied says what the host must do next: prepare, apply the graph, route, create, retire or save"]
pub struct Applied {
    /// The setup on disk is stale: save (debounced).
    pub save: bool,
    /// The engine's snapshot changed: derive it again and prepare.
    pub engine: bool,
    /// The backend's graph changed: a channel or its source kind, a device, a name.
    pub graph: bool,
    /// Which apps go to which channel changed: route them again.
    pub routing: bool,
    /// Instances the host must create (with their state, for a copy) before the engine lends.
    pub created: Vec<InstanceId>,
    /// Instances the host must retire: their inserts left the setup.
    pub removed: Vec<InstanceId>,
    /// The channel an `AddChannel` made.
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
}

/// Applies `cmd` to `setup` whole, or refuses it and leaves `setup` as it was.
pub fn apply(setup: &mut Setup, cmd: Command) -> Result<Applied, Refused> {
    match cmd {
        Command::AddChannel { kind } => add_channel(setup, kind),
        Command::RemoveChannel(ch) => {
            let at = setup
                .channels
                .iter()
                .position(|c| c.id == ch)
                .ok_or(Refused::NoChannel(ch))?;
            let gone = setup.channels.remove(at);
            Ok(Applied {
                engine: true,
                graph: true,
                routing: matches!(gone.source, Source::Apps(ref apps) if !apps.is_empty()),
                removed: gone.chain.iter().map(|i| i.instance).collect(),
                ..Applied::saved()
            })
        }
        Command::Rename { ch, name } => {
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(Refused::EmptyName);
            }
            let channel = channel_mut(setup, ch)?;
            if channel.name == name {
                return Ok(Applied::default());
            }
            channel.name = name;
            // The channel's PipeWire node describes itself by this name.
            Ok(Applied {
                graph: true,
                ..Applied::saved()
            })
        }
        Command::SetSource { ch, source } => set_source(setup, ch, source),
        Command::SetSend { ch, mix, position } => {
            let position = check_position(position)?;
            let channel = channel_mut(setup, ch)?;
            let before = channel.send(mix).position;
            channel.send_mut(mix).position = position;
            if channel.link {
                let other = match mix {
                    MixId::Monitor => MixId::Master,
                    MixId::Master => MixId::Monitor,
                };
                let send = channel.send_mut(other);
                send.position = (send.position + (position - before)).clamp(0.0, 1.0);
            }
            Ok(engine_change())
        }
        Command::SetMute { ch, mix, mute } => {
            channel_mut(setup, ch)?.send_mut(mix).mute = mute;
            Ok(engine_change())
        }
        Command::SetLink { ch, link } => {
            channel_mut(setup, ch)?.link = link;
            Ok(Applied::saved())
        }
        Command::SetChainRouting { ch, mix, on } => {
            let channel = channel_mut(setup, ch)?;
            match mix {
                MixId::Monitor => channel.chain_to_monitor = on,
                MixId::Master => channel.chain_to_master = on,
            }
            Ok(engine_change())
        }
        Command::AddInsert { strip, plugin, at } => {
            check_plugin(&plugin)?;
            let instance = next_instance(setup);
            let chain = chain_mut(setup, strip)?;
            if at > chain.len() {
                return Err(Refused::NoSlot {
                    strip,
                    index: at,
                    len: chain.len(),
                });
            }
            chain.insert(
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
            insert_mut(setup, instance)?.enabled = enabled;
            Ok(engine_change())
        }
        Command::SetInsertState { instance, state } => {
            insert_mut(setup, instance)?.state = state;
            Ok(Applied::saved())
        }
        Command::SetMixDevice { mix, device } => {
            if mix == MixId::Monitor && device.is_none() {
                return Err(Refused::MonitorNeedsDevice);
            }
            setup.mix_mut(mix).device = device;
            Ok(Applied {
                graph: true,
                ..Applied::saved()
            })
        }
        Command::SetMixLevel { mix, position } => {
            setup.mix_mut(mix).level.position = check_position(position)?;
            Ok(engine_change())
        }
        Command::SetMixMute { mix, mute } => {
            setup.mix_mut(mix).level.mute = mute;
            Ok(engine_change())
        }
        Command::SetPreview(on) => {
            setup.preview_master = on;
            Ok(engine_change())
        }
        Command::SetPrefs(prefs) => {
            if !Prefs::QUANTA.contains(&prefs.quantum) {
                return Err(Refused::NotAQuantum {
                    quantum: prefs.quantum,
                });
            }
            let graph = prefs.quantum != setup.prefs.quantum;
            setup.prefs = prefs;
            Ok(Applied {
                graph,
                ..Applied::saved()
            })
        }
    }
}

fn engine_change() -> Applied {
    Applied {
        engine: true,
        ..Applied::saved()
    }
}

fn add_channel(setup: &mut Setup, kind: ChannelKind) -> Result<Applied, Refused> {
    if setup.channels.len() >= MAX_CHANNELS {
        return Err(Refused::TooManyChannels { max: MAX_CHANNELS });
    }
    let id = ChannelId(setup.channels.iter().map(|c| c.id.0).max().unwrap_or(0) + 1);
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

fn set_source(setup: &mut Setup, ch: ChannelId, source: Source) -> Result<Applied, Refused> {
    let before = channel_mut(setup, ch)?.source.clone();
    let source = match source {
        Source::Apps(apps) => {
            // Ticking an app here takes it out of every other channel: an app plays into one
            // channel only (the prototype's checklist does the same).
            let mut unique: Vec<AppRef> = Vec::with_capacity(apps.len());
            for app in apps {
                if !unique.iter().any(|a| a.matches(&app)) {
                    unique.push(app);
                }
            }
            for other in setup.channels.iter_mut().filter(|c| c.id != ch) {
                if let Source::Apps(theirs) = &mut other.source {
                    theirs.retain(|a| !unique.iter().any(|mine| mine.matches(a)));
                }
            }
            Source::Apps(unique)
        }
        hardware => hardware,
    };
    if before == source {
        return Ok(Applied::default());
    }
    let kind_changed = std::mem::discriminant(&before) != std::mem::discriminant(&source);
    let routing = matches!(before, Source::Apps(_)) || matches!(source, Source::Apps(_));
    let graph = kind_changed || matches!(source, Source::Hardware(_));
    channel_mut(setup, ch)?.source = source;
    Ok(Applied {
        graph,
        routing,
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
        let mut item = chain_mut(setup, from.strip)?[from.index].clone();
        item.instance = next_instance(setup);
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
    Ok(engine_change())
}

fn check_position(position: f32) -> Result<f32, Refused> {
    if (0.0..=1.0).contains(&position) {
        Ok(position)
    } else {
        Err(Refused::NotAPosition(position))
    }
}

fn check_plugin(plugin: &PluginRef) -> Result<(), Refused> {
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

/// One more than the largest instance id in use. Lane 01 decides whether ids may be reused:
/// the host keeps a removed instance alive while its undo toast shows.
fn next_instance(setup: &Setup) -> InstanceId {
    InstanceId(setup.inserts().map(|i| i.instance.0).max().unwrap_or(0) + 1)
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

    fn names(chain: &[Insert]) -> Vec<&str> {
        chain.iter().map(|i| i.plugin.name.as_str()).collect()
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
        assert!(refused.to_string().contains("remove a channel"));
        assert_eq!(setup, before, "a refused command changes nothing");
    }

    #[test]
    fn an_app_ticked_in_channel_b_leaves_channel_a() {
        let mut setup = Setup::default();
        let a = apply(
            &mut setup,
            Command::AddChannel {
                kind: ChannelKind::Apps,
            },
        )
        .unwrap()
        .channel
        .unwrap();
        let b = apply(
            &mut setup,
            Command::AddChannel {
                kind: ChannelKind::Apps,
            },
        )
        .unwrap()
        .channel
        .unwrap();
        let _ = apply(
            &mut setup,
            Command::SetSource {
                ch: a,
                source: Source::Apps(vec![app("spotify"), app("firefox")]),
            },
        )
        .unwrap();
        let applied = apply(
            &mut setup,
            Command::SetSource {
                ch: b,
                source: Source::Apps(vec![app("spotify"), app("spotify")]),
            },
        )
        .unwrap();
        assert!(applied.routing);
        assert_eq!(
            setup.channel(a).unwrap().source,
            Source::Apps(vec![app("firefox")])
        );
        assert_eq!(
            setup.channel(b).unwrap().source,
            Source::Apps(vec![app("spotify")])
        );
        assert_eq!(setup.channel_of_app(&app("spotify")), Some(b));
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
        assert_eq!(setup, before);
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
        assert!(matches!(
            apply(
                &mut setup,
                Command::SetSend {
                    ch,
                    mix: MixId::Master,
                    position: f32::NAN,
                }
            ),
            Err(Refused::NotAPosition(_))
        ));
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
        assert_eq!(setup.channels[0].chain[0].state, Some(vec![1, 2, 3]));
        assert_eq!(names(&setup.channels[0].chain), ["C", "B", "A"]);
        assert_eq!(names(&setup.master.chain), ["C"]);
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
                device: None,
            },
        )
        .unwrap();
        assert!(applied.graph);
    }
}
