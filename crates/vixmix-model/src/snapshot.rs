//! The flat derivation of a [`Setup`] the engine prepares from: gains already linear, mutes,
//! the MON/MST routing, and each chain's instances in order. The engine never reads a
//! `Setup`; the host derives a snapshot after every command whose [`Applied`](crate::Applied)
//! says `engine` and hands it to `Engine::prepare`.

use crate::ids::{ChannelId, InstanceId, MixId};
use crate::setup::{Insert, Mix, Send, Setup};
use crate::taper::position_to_gain;

/// What the engine needs of the whole setup.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineSnapshot {
    /// The channels in the console's order; the backend's inputs arrive in the same order.
    pub channels: Vec<ChannelSnap>,
    pub monitor: MixSnap,
    pub master: MixSnap,
    /// Monitor's output plays the Master mix.
    pub preview_master: bool,
}

/// One channel as the engine runs it.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelSnap {
    pub id: ChannelId,
    pub monitor: SendSnap,
    pub master: SendSnap,
    /// MON: the Monitor send takes the chain's output.
    pub chain_to_monitor: bool,
    /// MST: the Master send takes the chain's output.
    pub chain_to_master: bool,
    pub chain: Vec<InsertSnap>,
}

/// A send or a mix level as the engine applies it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SendSnap {
    /// Linear amplitude: exactly 1.0 at unity (the engine skips that multiply), 0.0 silent.
    pub gain: f32,
    /// Mute beats everything: a muted send contributes nothing whatever its gain.
    pub mute: bool,
}

impl SendSnap {
    /// Whether this send contributes anything.
    pub fn audible(&self) -> bool {
        !self.mute && self.gain > 0.0
    }
}

/// One insert as the engine lends it.
#[derive(Clone, Debug, PartialEq)]
pub struct InsertSnap {
    pub instance: InstanceId,
    /// `vst3:<hex>`: what the rack is asked for beside the instance.
    pub plugin_id: String,
    /// Bypassed when false.
    pub enabled: bool,
}

/// One mix as the engine runs it.
#[derive(Clone, Debug, PartialEq)]
pub struct MixSnap {
    pub id: MixId,
    pub level: SendSnap,
    pub chain: Vec<InsertSnap>,
}

/// The engine's view of `setup`.
pub fn snapshot(setup: &Setup) -> EngineSnapshot {
    EngineSnapshot {
        channels: setup
            .channels
            .iter()
            .map(|c| ChannelSnap {
                id: c.id,
                monitor: send(&c.monitor),
                master: send(&c.master),
                chain_to_monitor: c.chain_to_monitor,
                chain_to_master: c.chain_to_master,
                chain: chain(&c.chain),
            })
            .collect(),
        monitor: mix(&setup.monitor),
        master: mix(&setup.master),
        preview_master: setup.preview_master,
    }
}

pub(crate) fn send(send: &Send) -> SendSnap {
    SendSnap {
        gain: position_to_gain(send.position),
        mute: send.mute,
    }
}

fn mix(mix: &Mix) -> MixSnap {
    MixSnap {
        id: mix.id,
        level: send(&mix.level),
        chain: chain(&mix.chain),
    }
}

fn chain(chain: &[Insert]) -> Vec<InsertSnap> {
    chain
        .iter()
        .map(|i| InsertSnap {
            instance: i.instance,
            plugin_id: i.plugin.id.clone(),
            enabled: i.enabled,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::example;
    use crate::taper::UNITY;

    #[test]
    fn unity_is_exactly_one_and_the_bottom_is_silence() {
        let mut setup = Setup::default();
        setup.channels[0].monitor.position = UNITY;
        setup.channels[0].master.position = 0.0;
        let snap = snapshot(&setup);
        assert_eq!(snap.channels[0].monitor.gain, 1.0);
        assert_eq!(snap.channels[0].master.gain, 0.0);
        assert!(!snap.channels[0].master.audible());
    }

    #[test]
    fn the_prototypes_setup_derives_in_order_with_its_mutes_and_routing() {
        let snap = snapshot(&example::setup());
        assert_eq!(snap.channels.len(), 6);
        // Music's Master send is muted; Voice chat's chain does not reach Monitor.
        assert!(snap.channels[2].master.mute);
        assert!(!snap.channels[4].chain_to_monitor);
        // Mic: three inserts in order; Master: two.
        assert_eq!(snap.channels[0].chain.len(), 3);
        assert_eq!(snap.master.chain.len(), 2);
        // Synth sends to Monitor at 0.64, which the taper reads as −8.57 dB.
        let gain = snap.channels[5].monitor.gain;
        assert!((gain - 10f32.powf(-8.571_428 / 20.0)).abs() < 1e-5);
    }
    #[test]
    fn every_part_the_engine_runs_reaches_the_snapshot() {
        let mut setup = example::setup();
        setup.preview_master = true;
        setup.master.level.mute = true;
        let snap = snapshot(&setup);
        let ids: Vec<u32> = snap.channels.iter().map(|c| c.id.0).collect();
        assert_eq!(ids, [1, 2, 3, 4, 5, 6], "the console's order");
        assert!(snap.preview_master);
        assert_eq!(snap.monitor.id, MixId::Monitor);
        assert!(snap.master.level.mute && !snap.master.level.audible());
        assert_eq!(snap.monitor.level.gain, 1.0);
        // Music's OTT: its instance, its plugin id and its bypass.
        assert_eq!(
            snap.channels[2].chain,
            [InsertSnap {
                instance: InstanceId(4),
                plugin_id: example::plugin("OTT").id,
                enabled: false,
            }]
        );
        let master: Vec<u32> = snap.master.chain.iter().map(|i| i.instance.0).collect();
        assert_eq!(master, [8, 9]);
        assert!(snap.channels[0].chain_to_master && !snap.channels[4].chain_to_monitor);
    }
}
