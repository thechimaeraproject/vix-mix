//! The undo of a removal: what the window's toast says, and the command that puts the removed
//! channel or insert back as it was (change request 2's `RestoreChannel` and `RestoreInsert`).
//! Both hosts read it from the setup just before they apply the removal.

use vixmix_model::{Command, Insert, MixId, Setup, SlotRef, Strip};

/// What a command is about to remove.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Removal {
    /// The toast's text.
    pub message: String,
    /// The command that puts it back.
    pub undo: Command,
    /// The inserts it takes away, whose instances the real host keeps while the toast shows.
    pub inserts: Vec<Insert>,
}

/// The removal `cmd` makes of `setup`, if it removes a channel or an insert that exists.
pub(crate) fn removal(setup: &Setup, cmd: &Command) -> Option<Removal> {
    match cmd {
        Command::RemoveChannel(ch) => {
            let at = setup.channels.iter().position(|c| c.id == *ch)?;
            let channel = setup.channels[at].clone();
            Some(Removal {
                message: format!("Removed channel {}", channel.name),
                inserts: channel.chain.clone(),
                undo: Command::RestoreChannel { at, channel },
            })
        }
        Command::RemoveInsert { instance } => {
            let (strip, index, insert) = find(setup, |i| i.instance == *instance)?;
            Some(Removal {
                message: format!(
                    "Removed {} from {}",
                    insert.plugin.name,
                    strip_name(setup, strip)
                ),
                inserts: vec![insert.clone()],
                undo: Command::RestoreInsert {
                    to: SlotRef { strip, index },
                    insert,
                },
            })
        }
        _ => None,
    }
}

/// The first insert `wanted` picks: its strip, index and a copy.
pub(crate) fn find(
    setup: &Setup,
    wanted: impl Fn(&Insert) -> bool,
) -> Option<(Strip, usize, Insert)> {
    let strips = setup
        .channels
        .iter()
        .map(|c| (Strip::Channel(c.id), &c.chain))
        .chain([
            (Strip::Mix(MixId::Monitor), &setup.monitor.chain),
            (Strip::Mix(MixId::Master), &setup.master.chain),
        ]);
    for (strip, chain) in strips {
        if let Some(index) = chain.iter().position(&wanted) {
            return Some((strip, index, chain[index].clone()));
        }
    }
    None
}

/// What the console calls `strip`: the channel's name, or the mix's.
pub(crate) fn strip_name(setup: &Setup, strip: Strip) -> String {
    match strip {
        Strip::Channel(ch) => setup
            .channel(ch)
            .map_or_else(|| format!("channel {}", ch.0), |c| c.name.clone()),
        Strip::Mix(mix) => mix.name().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::{apply, example};

    #[test]
    fn a_removals_undo_puts_back_exactly_what_it_took() {
        let mut setup = example::setup();
        let ch = setup.channels[0].id;
        let gone = removal(&setup, &Command::RemoveChannel(ch)).unwrap();
        assert_eq!(gone.message, "Removed channel Mic");
        assert_eq!(gone.inserts, setup.channels[0].chain);
        let before = setup.clone();
        let _ = apply(&mut setup, Command::RemoveChannel(ch)).unwrap();
        let restored = apply(&mut setup, gone.undo).unwrap();
        assert_eq!(setup.channels, before.channels);
        assert_eq!(
            restored.created,
            gone.inserts.iter().map(|i| i.instance).collect::<Vec<_>>()
        );

        let insert = setup.master.chain[0].clone();
        let gone = removal(
            &setup,
            &Command::RemoveInsert {
                instance: insert.instance,
            },
        )
        .unwrap();
        assert_eq!(
            gone.message,
            format!("Removed {} from Master", insert.plugin.name)
        );
        let before = setup.clone();
        let _ = apply(
            &mut setup,
            Command::RemoveInsert {
                instance: insert.instance,
            },
        )
        .unwrap();
        let _ = apply(&mut setup, gone.undo).unwrap();
        assert_eq!(setup, before);
        assert!(removal(&setup, &Command::SetPreview(true)).is_none());
    }
}
