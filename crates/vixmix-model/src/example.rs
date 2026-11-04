//! Prototype v5's example data (`docs/design/prototype/index.html`, *example data*) as a
//! [`Setup`] and the tables around it: the six channels with their apps, levels, mutes, link
//! flags and inserts, the two mixes, the apps the source panel lists, the devices and the
//! plugin catalog. `FakeHost` serves it to the app's tests and the look gate; nothing in a
//! real run reads it.

use crate::ids::{ChannelId, InstanceId, MixId};
use crate::setup::{
    AppRef, Channel, DeviceRef, Insert, Mix, PluginRef, Prefs, Send, Setup, Source,
};
use crate::taper::UNITY;

/// The prototype's `APPS`: id (used here as the binary), name, and whether it is running.
pub const APPS: [(&str, &str, bool); 13] = [
    ("firefox", "Firefox", true),
    ("chromium", "Chromium", false),
    ("spotify", "Spotify", true),
    ("feishin", "Feishin", true),
    ("mpv", "mpv", false),
    ("vlc", "VLC media player", false),
    ("discord", "Discord", true),
    ("vesktop", "Vesktop", false),
    ("steam", "Steam", true),
    ("hades", "Hades II", true),
    ("obs", "OBS Studio", true),
    ("bitwig", "Bitwig Studio", false),
    ("thunderbird", "Thunderbird", true),
];

/// The prototype's `HW_IN`: capture devices.
pub const INPUTS: [&str; 4] = [
    "Wave:3",
    "Scarlett 2i2 \u{b7} In 1",
    "Scarlett 2i2 \u{b7} In 2",
    "C920 webcam mic",
];

/// The prototype's `HW_OUT`: output devices.
pub const OUTPUTS: [&str; 4] = [
    "Scarlett 2i2 \u{b7} Phones",
    "Scarlett 2i2 \u{b7} Out 1\u{2013}2",
    "HDMI \u{b7} LG UltraGear",
    "Built-in speakers",
];

/// The prototype's `CATS`, in its order: the plugin browser's Type list.
pub const CATEGORIES: [&str; 10] = [
    "EQ",
    "Dynamics",
    "Reverb",
    "Delay",
    "Distortion",
    "Modulation",
    "Pitch & vocal",
    "Restoration",
    "Multi-effect",
    "Utility",
];

/// The prototype's `PLUG`: name, vendor, category (one of [`CATEGORIES`]).
pub const PLUGINS: [(&str, &str, &str); 42] = [
    ("Pro-Q 3", "FabFilter", "EQ"),
    ("Pro-L 2", "FabFilter", "Dynamics"),
    ("Pro-C 2", "FabFilter", "Dynamics"),
    ("Pro-DS", "FabFilter", "Dynamics"),
    ("Pro-G", "FabFilter", "Dynamics"),
    ("Pro-MB", "FabFilter", "Dynamics"),
    ("Pro-R 2", "FabFilter", "Reverb"),
    ("Saturn 2", "FabFilter", "Distortion"),
    ("Timeless 3", "FabFilter", "Delay"),
    ("OTT", "Xfer Records", "Dynamics"),
    ("ValhallaSupermassive", "Valhalla DSP", "Reverb"),
    ("ValhallaVintageVerb", "Valhalla DSP", "Reverb"),
    ("ValhallaDelay", "Valhalla DSP", "Delay"),
    ("RX 11 Voice De-noise", "iZotope", "Restoration"),
    ("Nectar 4", "iZotope", "Pitch & vocal"),
    ("Neutron 5", "iZotope", "Multi-effect"),
    ("TDR Nova", "Tokyo Dawn Labs", "EQ"),
    ("TDR Kotelnikov", "Tokyo Dawn Labs", "Dynamics"),
    ("Noise Suppression for Voice", "werman", "Restoration"),
    ("Airwindows Consolidated", "Airwindows", "Multi-effect"),
    ("Surge XT Effects", "Surge Synth Team", "Multi-effect"),
    ("Dragonfly Room Reverb", "Michael Willis", "Reverb"),
    ("Dragonfly Plate Reverb", "Michael Willis", "Reverb"),
    ("Dragonfly Hall Reverb", "Michael Willis", "Reverb"),
    ("Auto-Tune Pro", "Antares", "Pitch & vocal"),
    ("EchoBoy", "Soundtoys", "Delay"),
    ("Decapitator", "Soundtoys", "Distortion"),
    ("Vulf Compressor", "Goodhertz", "Dynamics"),
    ("Spaced Out", "BABY Audio", "Reverb"),
    ("Crystalline", "BABY Audio", "Reverb"),
    ("Youlean Loudness Meter 2", "Youlean", "Utility"),
    ("MAutoPitch", "MeldaProduction", "Pitch & vocal"),
    ("MEqualizer", "MeldaProduction", "EQ"),
    ("ShaperBox 3", "Cableguys", "Modulation"),
    ("Kickstart 2", "Cableguys", "Dynamics"),
    ("Comp FET-76", "Arturia", "Dynamics"),
    ("Supercharger", "Native Instruments", "Dynamics"),
    ("Snap Heap", "Kilohearts", "Multi-effect"),
    ("kHs Chorus", "Kilohearts", "Modulation"),
    ("kHs Gate", "Kilohearts", "Dynamics"),
    ("SPAN", "Voxengo", "Utility"),
    ("Gullfoss", "Soundtheory", "EQ"),
];

/// The prototype's favourites, by name.
pub const FAVORITES: [&str; 4] = ["Pro-Q 3", "Noise Suppression for Voice", "OTT", "Pro-L 2"];

/// The prototype's recent list, newest first, by name.
pub const RECENT: [&str; 5] = [
    "Pro-L 2",
    "OTT",
    "Pro-Q 3",
    "TDR Nova",
    "ValhallaSupermassive",
];

/// The prototype's meter `style` per channel, in channel order: what drives `FakeHost`'s
/// simulated signal (`speech`, `game`, `steady`, `low`, `burst`).
pub const STYLES: [&str; 6] = ["speech", "game", "steady", "low", "burst", "burst"];

/// The example app with `id`, as a setup stores it.
pub fn app(id: &str) -> AppRef {
    let name = APPS
        .iter()
        .find(|(app, _, _)| *app == id)
        .map_or(id, |(_, name, _)| name);
    AppRef {
        app_id: None,
        binary: Some(id.to_string()),
        name: name.to_string(),
    }
}

/// An example device: its description is the prototype's label, its `node.name` a slug of it.
pub fn device(description: &str, input: bool) -> DeviceRef {
    let slug: String = description
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    let prefix = if input { "alsa_input" } else { "alsa_output" };
    DeviceRef {
        name: format!("{prefix}.{slug}"),
        description: description.to_string(),
    }
}

/// An example plugin reference: a made-up but well-formed class id derived from the name.
pub fn plugin(name: &str) -> PluginRef {
    // FNV-1a, four times with different seeds, for 32 stable hex digits.
    let word = |seed: u32| {
        name.bytes().fold(0x811c_9dc5 ^ seed, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        })
    };
    PluginRef {
        id: format!(
            "vst3:{:08X}{:08X}{:08X}{:08X}",
            word(1),
            word(2),
            word(3),
            word(4)
        ),
        bundle: format!("{name}.vst3"),
        name: name.to_string(),
    }
}

/// The prototype's console: Mic, Game, Music, Browser, Voice chat and Synth; Monitor with
/// Pro-Q 3 to the phones; Master with Pro-Q 3 and Pro-L 2 to the main outputs.
pub fn setup() -> Setup {
    let mut next = 0;
    let mut chain = |plugins: &[(&str, bool)]| -> Vec<Insert> {
        plugins
            .iter()
            .map(|(name, enabled)| {
                next += 1;
                Insert {
                    instance: InstanceId(next),
                    plugin: plugin(name),
                    enabled: *enabled,
                    state: None,
                }
            })
            .collect()
    };
    let send = |position: f32, mute: bool| Send { position, mute };
    let apps = |ids: &[&str]| Source::Apps(ids.iter().map(|id| app(id)).collect());
    #[allow(clippy::too_many_arguments)]
    fn channel(
        id: u32,
        name: &str,
        source: Source,
        monitor: Send,
        master: Send,
        link: bool,
        chain_to_monitor: bool,
        chain: Vec<Insert>,
    ) -> Channel {
        Channel {
            id: ChannelId(id),
            name: name.to_string(),
            source,
            chain,
            chain_to_monitor,
            chain_to_master: true,
            monitor,
            master,
            link,
        }
    }
    let channels = vec![
        channel(
            1,
            "Mic",
            Source::Hardware(Some(device(INPUTS[0], true))),
            send(0.76, false),
            send(UNITY, false),
            false,
            true,
            chain(&[
                ("Noise Suppression for Voice", true),
                ("Pro-C 2", true),
                ("Pro-L 2", true),
            ]),
        ),
        channel(
            2,
            "Game",
            apps(&["hades", "steam"]),
            send(0.74, false),
            send(0.64, false),
            true,
            true,
            Vec::new(),
        ),
        channel(
            3,
            "Music",
            apps(&["spotify", "feishin", "mpv", "vlc"]),
            send(0.68, false),
            send(0.58, true),
            false,
            true,
            chain(&[("OTT", false)]),
        ),
        channel(
            4,
            "Browser",
            apps(&["firefox", "chromium"]),
            send(0.72, false),
            send(0.6, false),
            true,
            true,
            Vec::new(),
        ),
        channel(
            5,
            "Voice chat",
            apps(&["discord", "vesktop"]),
            send(0.78, false),
            send(0.62, false),
            false,
            false,
            chain(&[("TDR Nova", true)]),
        ),
        channel(
            6,
            "Synth",
            Source::Hardware(Some(device(INPUTS[2], true))),
            send(0.64, false),
            send(0.64, false),
            true,
            true,
            chain(&[("ValhallaSupermassive", true)]),
        ),
    ];
    let monitor = Mix {
        device: Some(device(OUTPUTS[0], false)),
        chain: chain(&[("Pro-Q 3", true)]),
        ..Mix::new(MixId::Monitor)
    };
    let master = Mix {
        device: Some(device(OUTPUTS[1], false)),
        chain: chain(&[("Pro-Q 3", true), ("Pro-L 2", true)]),
        ..Mix::new(MixId::Master)
    };
    Setup {
        channels,
        monitor,
        master,
        preview_master: false,
        prefs: Prefs::default(),
        version: crate::persist::VERSION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_is_the_prototypes_console() {
        let setup = setup();
        let names: Vec<&str> = setup.channels.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["Mic", "Game", "Music", "Browser", "Voice chat", "Synth"]
        );
        assert!(
            setup.channels[2].master.mute,
            "Music's Master send is muted"
        );
        assert!(
            !setup.channels[2].chain[0].enabled,
            "Music's OTT is bypassed"
        );
        assert!(!setup.channels[4].chain_to_monitor, "Voice chat: MON off");
        let links: Vec<bool> = setup.channels.iter().map(|c| c.link).collect();
        assert_eq!(links, [false, true, false, true, false, true]);
        // Every app in at most one channel, as apply keeps it.
        for (id, _, _) in APPS {
            let owners = setup
                .channels
                .iter()
                .filter(|c| matches!(&c.source, Source::Apps(a) if a.contains(&app(id))))
                .count();
            assert!(owners <= 1, "{id} is in {owners} channels");
        }
        // Instance ids are unique across every chain.
        let mut ids: Vec<u32> = setup.inserts().map(|i| i.instance.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), setup.inserts().count());
    }

    #[test]
    fn every_example_plugin_id_is_one_a_setup_accepts() {
        for (name, _, category) in PLUGINS {
            let id = plugin(name).id;
            let hex = id.strip_prefix("vst3:").unwrap();
            assert_eq!(hex.len(), 32);
            assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(CATEGORIES.contains(&category), "{name}: {category}");
        }
    }
}
