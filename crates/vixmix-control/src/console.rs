//! What a request is asked against: the setup, and the devices the machine has now.
//!
//! A setup names the device a mix plays to and the one a hardware channel captures (a
//! `DeviceRef`: a node name and a description), whether or not that device is plugged in.
//! Which devices are there now is the host's to know, not the setup's, so a [`Console`]
//! carries both. A `Setup` alone is a console with no device present, which is all a test of
//! the switches and faders needs.

use vixmix_model::{DeviceDirection, DeviceInfo, DeviceRef, Setup, Source, Strip, slug};

/// A setup with the devices present now.
#[derive(Clone, Copy, Debug)]
pub struct Console<'a> {
    pub setup: &'a Setup,
    /// The devices the machine has now, inputs and outputs, as the host lists them.
    pub devices: &'a [DeviceInfo],
}

impl<'a> From<&'a Setup> for Console<'a> {
    /// The setup with no device present.
    fn from(setup: &'a Setup) -> Console<'a> {
        Console {
            setup,
            devices: &[],
        }
    }
}

impl<'a> Console<'a> {
    /// The devices present in `direction`, in the host's order, each with the word it is
    /// listed under: its node name as one plain word (`slug`), with a number after it when
    /// two names make the same word.
    pub fn listed(&self, direction: DeviceDirection) -> Vec<(String, &'a DeviceRef)> {
        let mut out: Vec<(String, &'a DeviceRef)> = Vec::new();
        for info in self.devices.iter().filter(|d| d.direction == direction) {
            let word = slug(&info.device.name);
            let mut key = word.clone();
            let mut count = 1;
            while out.iter().any(|(taken, _)| *taken == key) {
                count += 1;
                key = format!("{word}-{count}");
            }
            out.push((key, &info.device));
        }
        out
    }

    /// Whether `device` is there now, in `direction`: a device is the same one when its node
    /// name is.
    pub fn present(&self, direction: DeviceDirection, device: &DeviceRef) -> bool {
        self.devices
            .iter()
            .any(|d| d.direction == direction && d.device.name == device.name)
    }

    /// Which way the device of `strip` goes, and the device the setup names for it: a mix's
    /// output, a hardware channel's input. `None` for a strip with no device of its own: an
    /// app channel, or a channel that is not there.
    pub fn device(&self, strip: Strip) -> Option<(DeviceDirection, Option<&'a DeviceRef>)> {
        match strip {
            Strip::Mix(mix) => Some((DeviceDirection::Output, self.setup.mix(mix).device.as_ref())),
            Strip::Channel(ch) => match &self.setup.channel(ch)?.source {
                Source::Hardware(device) => Some((DeviceDirection::Input, device.as_ref())),
                Source::Apps(_) => None,
            },
        }
    }
}

/// The example console's devices, as its host lists them: four inputs, then four outputs.
#[cfg(test)]
pub(crate) fn example_devices() -> Vec<DeviceInfo> {
    use vixmix_model::example;
    let inputs = example::INPUTS
        .iter()
        .map(|d| (d, DeviceDirection::Input, true));
    let outputs = example::OUTPUTS
        .iter()
        .map(|d| (d, DeviceDirection::Output, false));
    inputs
        .chain(outputs)
        .map(|(description, direction, input)| DeviceInfo {
            device: example::device(description, input),
            direction,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::{ChannelId, MixId, example};

    fn info(name: &str, description: &str, direction: DeviceDirection) -> DeviceInfo {
        DeviceInfo {
            device: DeviceRef {
                name: name.to_string(),
                description: description.to_string(),
            },
            direction,
        }
    }

    #[test]
    fn a_device_is_listed_under_its_node_name_as_one_word_each_way_apart() {
        let setup = example::setup();
        let devices = [
            info(
                "alsa_output.usb-Scarlett_2i2-00.analog-stereo",
                "Scarlett 2i2",
                DeviceDirection::Output,
            ),
            info(
                "alsa_input.usb-Scarlett_2i2-00.analog-stereo",
                "Scarlett 2i2",
                DeviceDirection::Input,
            ),
            info(
                "bluez_output.AA_BB.1",
                "Headphones",
                DeviceDirection::Output,
            ),
            // Two names that make the same word: the second is numbered.
            info(
                "bluez_output.aa-bb.1",
                "Headphones (2)",
                DeviceDirection::Output,
            ),
        ];
        let console = Console {
            setup: &setup,
            devices: &devices,
        };
        let words = |direction| -> Vec<String> {
            console
                .listed(direction)
                .into_iter()
                .map(|(word, _)| word)
                .collect()
        };
        assert_eq!(
            words(DeviceDirection::Output),
            [
                "alsa-output-usb-scarlett-2i2-00-analog-stereo",
                "bluez-output-aa-bb-1",
                "bluez-output-aa-bb-1-2",
            ]
        );
        assert_eq!(
            words(DeviceDirection::Input),
            ["alsa-input-usb-scarlett-2i2-00-analog-stereo"]
        );
        // Present is by node name, and each way apart.
        assert!(console.present(DeviceDirection::Output, &devices[2].device));
        assert!(!console.present(DeviceDirection::Input, &devices[2].device));
    }

    #[test]
    fn a_mix_and_a_hardware_channel_have_a_device_and_an_app_channel_has_none() {
        let setup = example::setup();
        let console = Console::from(&setup);
        // Monitor plays to the phones, Mic captures the Wave:3; Game is apps.
        let (way, device) = console.device(Strip::Mix(MixId::Monitor)).unwrap();
        assert_eq!(way, DeviceDirection::Output);
        assert_eq!(device.unwrap().description, example::OUTPUTS[0]);
        let (way, device) = console.device(Strip::Channel(ChannelId(1))).unwrap();
        assert_eq!(way, DeviceDirection::Input);
        assert_eq!(device.unwrap().description, example::INPUTS[0]);
        assert!(console.device(Strip::Channel(ChannelId(2))).is_none());
        assert!(console.device(Strip::Channel(ChannelId(99))).is_none());
        // A setup alone has no device present.
        assert!(console.listed(DeviceDirection::Output).is_empty());
    }
}
