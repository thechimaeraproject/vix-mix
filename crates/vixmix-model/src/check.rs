//! What a setup read from `setup.ron` must hold before the rest of vix-mix trusts it: the
//! rules [`apply`](crate::apply) keeps, checked once at load, because a hand edit can break
//! any of them and the engine, the backend and the host all count on them.

use crate::command::{Refused, check_plugin};
use crate::ids::{ChannelId, InstanceId, MixId};
use crate::setup::{AppRef, MAX_CHANNELS, Prefs, Send, Setup, Source};

/// Why `setup.ron` holds a setup no command could have made. Each message names what is wrong
/// and how to fix the file; `persist::load` falls back to the default with it.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Invalid {
    #[error(
        "it has {found} channels and vix-mix mixes at most {max}: remove channels until {max} are left"
    )]
    TooManyChannels { found: usize, max: usize },
    #[error("two channels have id {0:?}: give each channel an id of its own")]
    SameChannel(ChannelId),
    #[error("two inserts have instance {0:?}: give each insert an instance id of its own")]
    SameInstance(InstanceId),
    #[error("channel {0:?} has no name: give it one")]
    NoName(ChannelId),
    #[error(
        "{app:?} is listed twice, in channel {first:?} and in channel {second:?}: an app plays into one channel only, so remove one of them"
    )]
    AppTwice {
        app: String,
        first: ChannelId,
        second: ChannelId,
    },
    #[error(
        "{fader} is at {position}, which is not a fader position: set it between 0 and 1 (0.84 is unity)"
    )]
    NotAPosition { fader: String, position: f32 },
    #[error("the `{field}` mix says `id: {found:?}`: set it to `id: {expected:?}`")]
    WrongMix {
        field: &'static str,
        found: MixId,
        expected: MixId,
    },
    #[error("insert {instance:?}: {refused}")]
    Plugin {
        instance: InstanceId,
        refused: Refused,
    },
    #[error("the quantum is {0}, which the preferences do not offer: set it to 128, 256 or 512")]
    NotAQuantum(u32),
    #[error(
        "it lists {found} plugin folders and vix-mix scans at most {max}: remove folders until {max} are left"
    )]
    TooManyPluginDirs { found: usize, max: usize },
}

/// Whether `setup` keeps every rule `apply` keeps: at most [`MAX_CHANNELS`] channels, ids
/// unique, every channel named, an app in one channel only, faders in 0..=1, plugins stored by
/// id and bundle name, the mixes in their places, a quantum the preferences offer and at most
/// [`Prefs::MAX_PLUGIN_DIRS`] plugin folders. The id counters are not checked: `persist`
/// raises them instead.
pub(crate) fn check(setup: &Setup) -> Result<(), Invalid> {
    if setup.channels.len() > MAX_CHANNELS {
        return Err(Invalid::TooManyChannels {
            found: setup.channels.len(),
            max: MAX_CHANNELS,
        });
    }
    for (field, mix, expected) in [
        ("monitor", &setup.monitor, MixId::Monitor),
        ("master", &setup.master, MixId::Master),
    ] {
        if mix.id != expected {
            return Err(Invalid::WrongMix {
                field,
                found: mix.id,
                expected,
            });
        }
        fader(format!("{}'s level", expected.name()), &mix.level)?;
    }
    let mut channels: Vec<ChannelId> = Vec::with_capacity(setup.channels.len());
    let mut apps: Vec<(ChannelId, &AppRef)> = Vec::new();
    for channel in &setup.channels {
        if channels.contains(&channel.id) {
            return Err(Invalid::SameChannel(channel.id));
        }
        channels.push(channel.id);
        if channel.name.trim().is_empty() {
            return Err(Invalid::NoName(channel.id));
        }
        for mix in [MixId::Monitor, MixId::Master] {
            let name = format!("{}'s {} send", channel.name, mix.name());
            fader(name, channel.send(mix))?;
        }
        if let Source::Apps(list) = &channel.source {
            for app in list {
                if let Some((first, _)) = apps.iter().find(|(_, other)| other.matches(app)) {
                    return Err(Invalid::AppTwice {
                        app: app.name.clone(),
                        first: *first,
                        second: channel.id,
                    });
                }
                apps.push((channel.id, app));
            }
        }
    }
    let mut instances: Vec<InstanceId> = Vec::new();
    for insert in setup.inserts() {
        if instances.contains(&insert.instance) {
            return Err(Invalid::SameInstance(insert.instance));
        }
        instances.push(insert.instance);
        check_plugin(&insert.plugin).map_err(|refused| Invalid::Plugin {
            instance: insert.instance,
            refused,
        })?;
    }
    if !Prefs::QUANTA.contains(&setup.prefs.quantum) {
        return Err(Invalid::NotAQuantum(setup.prefs.quantum));
    }
    if setup.prefs.plugin_dirs.len() > Prefs::MAX_PLUGIN_DIRS {
        return Err(Invalid::TooManyPluginDirs {
            found: setup.prefs.plugin_dirs.len(),
            max: Prefs::MAX_PLUGIN_DIRS,
        });
    }
    Ok(())
}

fn fader(name: String, send: &Send) -> Result<(), Invalid> {
    if (0.0..=1.0).contains(&send.position) {
        Ok(())
    } else {
        Err(Invalid::NotAPosition {
            fader: name,
            position: send.position,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::example;
    use crate::setup::Insert;

    fn refused_by(edit: impl FnOnce(&mut Setup)) -> Invalid {
        let mut setup = example::setup();
        edit(&mut setup);
        check(&setup).unwrap_err()
    }

    #[test]
    fn what_commands_make_passes() {
        check(&example::setup()).unwrap();
        check(&Setup::default()).unwrap();
    }

    #[test]
    fn a_hand_edit_that_breaks_a_rule_is_named_with_its_fix() {
        let ninth = refused_by(|s| {
            let template = s.channels[0].clone();
            for id in 10..13 {
                s.channels.push(crate::setup::Channel {
                    id: ChannelId(id),
                    chain: Vec::new(),
                    ..template.clone()
                });
            }
        });
        assert_eq!(
            ninth,
            Invalid::TooManyChannels { found: 9, max: 8 },
            "{ninth}"
        );
        assert!(
            ninth
                .to_string()
                .contains("remove channels until 8 are left")
        );

        let twins = refused_by(|s| s.channels[1].id = s.channels[0].id);
        assert_eq!(twins, Invalid::SameChannel(ChannelId(1)));

        let shared = refused_by(|s| {
            let insert: Insert = s.channels[0].chain[0].clone();
            s.master.chain.push(insert);
        });
        assert_eq!(shared, Invalid::SameInstance(InstanceId(1)));

        let steam = refused_by(|s| {
            if let Source::Apps(apps) = &mut s.channels[2].source {
                apps.push(example::app("steam"));
            }
        });
        assert!(
            steam
                .to_string()
                .contains("an app plays into one channel only"),
            "{steam}"
        );

        let loud = refused_by(|s| s.channels[3].monitor.position = 1.5);
        assert!(
            loud.to_string()
                .starts_with("Browser's Monitor send is at 1.5")
        );
        let nan = refused_by(|s| s.master.level.position = f32::NAN);
        assert!(matches!(nan, Invalid::NotAPosition { .. }));

        let swapped = refused_by(|s| s.monitor.id = MixId::Master);
        assert!(swapped.to_string().contains("set it to `id: Monitor`"));

        let unnamed = refused_by(|s| s.channels[4].name = "  ".to_string());
        assert_eq!(unnamed, Invalid::NoName(ChannelId(5)));

        let path = refused_by(|s| s.monitor.chain[0].plugin.bundle = "/x/EQ.vst3".to_string());
        assert!(path.to_string().contains("by its file name"), "{path}");

        let quantum = refused_by(|s| s.prefs.quantum = 64);
        assert!(quantum.to_string().contains("128, 256 or 512"));

        let folders = refused_by(|s| {
            s.prefs.plugin_dirs = (0..=Prefs::MAX_PLUGIN_DIRS)
                .map(|i| format!("/opt/vst3/{i}").into())
                .collect();
        });
        assert_eq!(
            folders,
            Invalid::TooManyPluginDirs { found: 17, max: 16 },
            "{folders}"
        );
        assert!(
            folders
                .to_string()
                .contains("remove folders until 16 are left")
        );
    }

    #[test]
    fn sixteen_plugin_folders_pass() {
        let mut setup = example::setup();
        setup.prefs.plugin_dirs = (0..Prefs::MAX_PLUGIN_DIRS)
            .map(|i| format!("/opt/vst3/{i}").into())
            .collect();
        check(&setup).unwrap();
    }
}
