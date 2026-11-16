//! The control from outside, in the window: the socket other programs reach the mixer by,
//! served against the window's host on the window's thread.
//!
//! `vixmix-control` holds the protocol, the clients and the socket; what is here is when.
//! [`Control::open`] opens the socket of this run's profile once the host has booted.
//! [`Control::serve`] answers the requests that wait: the event loop is woken for it by its
//! own event ([`Asked::Control`](crate::app::Asked)), sent once however many wait. A request
//! is applied with [`Host::apply`] and nothing else: no toast (a refusal is the client's to
//! read, on its own line) and no undo, as a fader moved in the window has none. [`poll`] is
//! the one place the window polls its host, and it tells the socket's followers when the
//! poll reported a change to the setup or to the devices the machine has, so a gesture in the
//! window, or a device unplugged, reaches a controller as a request from another controller
//! does. A request that names a device is held to the devices the host lists.
//!
//! On a platform without Unix sockets there is no socket and nothing is said about it: the
//! service is made and nobody can reach it.

#[cfg(test)]
use vixmix_control::Connector;
use vixmix_control::{Console, SERVE_BUDGET, Served, Service, Target, service};
use vixmix_host::{Host, HostEvent};
use vixmix_model::{Applied, Command, DeviceInfo, Refused, Setup};

/// The profile name `--fake` runs under: its console is an example, so it never takes the
/// socket of a mixer that plays.
pub const FAKE_PROFILE: &str = "fake";

/// The host as a request needs it: its setup, the devices it lists, and a command applied
/// to it.
struct Desk<'a>(&'a mut dyn Host);

impl Target for Desk<'_> {
    fn setup(&self) -> &Setup {
        self.0.setup()
    }
    fn devices(&self) -> &[DeviceInfo] {
        self.0.devices()
    }
    fn apply(&mut self, command: Command) -> Result<Applied, Refused> {
        self.0.apply(command)
    }
}

/// The control socket and its clients, for as long as the window runs.
pub struct Control {
    service: Service,
    /// The socket, while it is open; dropping it closes every connection and removes the
    /// file.
    #[cfg(unix)]
    _listener: Option<vixmix_control::Listener>,
}

impl Control {
    /// Opens the socket of the mixer that runs under `profile`. `wake` is called from the
    /// socket's threads when a request waits and the window is not already owed a wake; it
    /// must not block.
    ///
    /// The second answer is why the socket is not open, when it is not: for the toast line,
    /// since it says what to do. The window runs the same without it.
    pub fn open(
        profile: &str,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> (Control, Option<String>) {
        let (service, connector) = service(wake);
        #[cfg(unix)]
        {
            let opened = vixmix_control::socket_path_from_env(profile).and_then(|path| {
                vixmix_control::Listener::open(&path, connector).map_err(|e| e.to_string())
            });
            let (listener, problem) = match opened {
                Ok(listener) => {
                    log::info!(
                        "the control socket is {}: `vix-mix ctl get /` lists what it carries",
                        listener.path().display()
                    );
                    (Some(listener), None)
                }
                Err(problem) => (None, Some(problem)),
            };
            (
                Control {
                    service,
                    _listener: listener,
                },
                problem,
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (profile, connector);
            (Control { service }, None)
        }
    }

    /// A control with no socket, and the connector that brings clients to it: what the
    /// headless tests drive, as a socket's threads would.
    #[cfg(test)]
    pub fn in_memory() -> (Control, Connector) {
        let (service, connector) = service(|| {});
        (
            Control {
                service,
                #[cfg(unix)]
                _listener: None,
            },
            connector,
        )
    }

    /// Answers the requests that wait, against `host`. `changed` says a request changed the
    /// setup: the window then draws, or, hidden, polls its host ([`poll`]).
    pub fn serve(&mut self, host: &mut dyn Host) -> Served {
        self.service.serve(SERVE_BUDGET, &mut Desk(host))
    }
}

/// Polls `host`, and tells the socket's followers what changed when the poll reported a
/// change to the setup or to the devices the machine has. Every frame and every tick between
/// frames polls through here.
pub fn poll(host: &mut dyn Host, control: &mut Control) -> Vec<HostEvent> {
    let events = host.poll();
    let changed = events
        .iter()
        .any(|event| matches!(event, HostEvent::SetupChanged | HostEvent::Devices(_)));
    if changed {
        control.service.publish(Console {
            setup: host.setup(),
            devices: host.devices(),
        });
    }
    events
}

/// The name the socket is opened under: the configuration profile's, so a debug build's
/// mixer and the installed one each have their own, or [`FAKE_PROFILE`] for `--fake`.
pub fn profile_name(fake: bool) -> String {
    if fake {
        return FAKE_PROFILE.to_string();
    }
    vixmix_model::persist::profile(cfg!(debug_assertions))
        .map_or_else(|| "default".to_string(), |profile| profile.name)
}

#[cfg(test)]
mod tests {
    use egui::Pos2;
    use vixmix_control::{Client, ERROR};
    use vixmix_model::{ChannelId, MixId, Source, Strip, example};

    use super::*;
    use crate::fader::Target as Fader;
    use crate::test_support::Harness;

    /// A client of the harness's control, greeted.
    fn client(h: &mut Harness) -> Client {
        let client = h.connector.open().expect("room for a client");
        h.heard();
        let greeting = said(&client);
        assert_eq!(greeting, [vixmix_control::greeting()]);
        client
    }

    /// Every line `client` was sent and has not read.
    fn said(client: &Client) -> Vec<String> {
        client
            .answers
            .try_iter()
            .flat_map(|message| message.lines().map(str::to_string).collect::<Vec<_>>())
            .collect()
    }

    /// One request, served as the window serves it, and its answer.
    fn ask(h: &mut Harness, client: &Client, line: &str) -> Vec<String> {
        assert!(client.requests.send(line));
        h.heard();
        said(client)
    }

    /// Where a send's or a mix's mute button is.
    fn mute_at(h: &Harness, target: Fader) -> Pos2 {
        let column = h.fader(target);
        let row = match target {
            Fader::Send { ch, .. } => h.buttons(Strip::Channel(ch)),
            Fader::Level(mix) => h.buttons(Strip::Mix(mix)),
        };
        Pos2::new(column.center().x, row.center().y)
    }

    #[test]
    fn a_request_for_a_mute_sends_the_command_the_mute_button_sends() {
        let mut h = Harness::new();
        let controller = client(&mut h);
        let mic = h.channel(0);
        let target = Fader::Send {
            ch: mic,
            mix: MixId::Monitor,
        };
        // The button in the window, then the same switch from outside: one command each,
        // the same but for the way it now points.
        let pressed = h.click(mute_at(&h, target));
        assert_eq!(
            pressed,
            [Command::SetMute {
                ch: mic,
                mix: MixId::Monitor,
                mute: true
            }]
        );
        let from = h.host.calls.len();
        assert_eq!(
            ask(&mut h, &controller, "toggle /ch/mic/monitor/mute"),
            ["/ch/1/monitor/mute false", "ok"]
        );
        assert_eq!(
            h.host.sent_since(from),
            [Command::SetMute {
                ch: mic,
                mix: MixId::Monitor,
                mute: false
            }]
        );
        // A plugin's bypass and a fader, by the names a person reads on the console.
        let from = h.host.calls.len();
        assert_eq!(
            ask(&mut h, &controller, "set /ch/mic/fx/pro-c-2/enabled false"),
            ["/fx/2/enabled false", "ok"]
        );
        assert_eq!(
            ask(&mut h, &controller, "set /mix/monitor/db -12")
                .last()
                .unwrap(),
            "ok"
        );
        assert_eq!(
            h.host.sent_since(from),
            [
                Command::SetInsertEnabled {
                    instance: vixmix_model::InstanceId(2),
                    enabled: false
                },
                Command::SetMixLevel {
                    mix: MixId::Monitor,
                    position: 0.56
                },
            ]
        );
    }

    #[test]
    fn a_change_from_outside_shows_in_the_window_at_its_next_frame() {
        let mut h = Harness::new();
        let controller = client(&mut h);
        assert_eq!(
            h.find("-12.0"),
            None,
            "no fader reads -12 dB in the example"
        );
        assert!(controller.requests.send("set /mix/monitor/db -12"));
        let served = h.heard();
        assert!(served.changed, "the window is told to draw");
        h.idle(1);
        assert!(
            h.find("-12.0").is_some(),
            "Monitor's readout: {:?}",
            h.drawn()
        );
        // A read changes nothing, so it asks for no frame.
        assert!(controller.requests.send("get /mix/monitor/db"));
        assert!(!h.heard().changed);
    }

    #[test]
    fn a_refused_request_is_the_clients_to_read_and_shows_no_toast() {
        let mut h = Harness::new();
        let controller = client(&mut h);
        let from = h.host.calls.len();
        for (line, expected) in [
            ("set /ch/9/mute true", "there is no channel 9"),
            // Refused by the host's model, in its words.
            (
                "set /ch/mic/monitor/position 2",
                "clamp it to 0..=1 before sending",
            ),
        ] {
            let answer = ask(&mut h, &controller, line);
            assert_eq!(answer.len(), 1, "{answer:?}");
            assert!(
                answer[0].starts_with(ERROR) && answer[0].contains(expected),
                "{line} answered {answer:?}"
            );
        }
        // The first never reached the host; the second did and was refused there.
        assert_eq!(
            h.host.sent_since(from),
            [Command::SetSend {
                ch: ChannelId(1),
                mix: MixId::Monitor,
                position: 2.0
            }]
        );
        h.idle(1);
        assert_eq!(
            h.shell.toasts.current(h.shell.now),
            None,
            "the toast line stays empty"
        );
    }

    #[test]
    fn a_click_in_the_window_reaches_a_client_that_follows_it() {
        let mut h = Harness::new();
        let follower = client(&mut h);
        // Mic's Monitor send is at 0.76, 0.06 above the taper's -6 dB at 0.70, where 0.14 of
        // travel is 6 dB: -6 + 2.57.
        assert_eq!(
            ask(&mut h, &follower, "watch /ch/mic"),
            [
                "/ch/1/name \"Mic\"",
                "/ch/1/kind \"hardware\"",
                "/ch/1/link false",
                "/ch/1/mute false",
                "/ch/1/monitor/mute false",
                "/ch/1/monitor/db -3.43",
                "/ch/1/monitor/position 0.76",
                "/ch/1/monitor/processed true",
                "/ch/1/master/mute false",
                "/ch/1/master/db 0",
                "/ch/1/master/position 0.84",
                "/ch/1/master/processed true",
                "/ch/1/device/name \"alsa_input.wave_3\"",
                "/ch/1/device/description \"Wave:3\"",
                "/ch/1/device/present true",
                "ok",
            ]
        );
        let mic = h.channel(0);
        let pressed = h.click(mute_at(
            &h,
            Fader::Send {
                ch: mic,
                mix: MixId::Master,
            },
        ));
        assert_eq!(pressed.len(), 1, "{pressed:?}");
        // The frame after the click polled the host, which reported the change.
        assert_eq!(said(&follower), ["event /ch/1/master/mute true"]);
        // Music's mute is not under what it follows.
        let music = h.channel(2);
        let _ = h.click(mute_at(
            &h,
            Fader::Send {
                ch: music,
                mix: MixId::Monitor,
            },
        ));
        assert_eq!(said(&follower), Vec::<String>::new());
    }

    #[test]
    fn fake_runs_under_a_name_of_its_own() {
        assert_eq!(profile_name(true), FAKE_PROFILE);
        assert_ne!(profile_name(false), FAKE_PROFILE);
    }

    #[test]
    fn a_request_for_a_device_sends_the_command_its_panel_sends() {
        let mut h = Harness::new();
        let controller = client(&mut h);
        let speakers = example::device("Built-in speakers", false);
        // The output panel in the window: Master to the built-in speakers.
        let _ = h.click(h.head(Strip::Mix(MixId::Master)).center());
        h.idle(2);
        let picked = h.click(h.text("Built-in speakers"));
        assert_eq!(
            picked,
            [Command::SetMixDevice {
                mix: MixId::Master,
                device: Some(speakers.clone())
            }]
        );
        let _ = h.click(h.text("Done"));
        // The same from outside for Monitor, written as the device is described: the same
        // device, the host's own, in the same command.
        let from = h.host.calls.len();
        assert_eq!(
            ask(
                &mut h,
                &controller,
                "set /mix/monitor/device/name built-in-speakers"
            ),
            [
                "/mix/monitor/device/name \"alsa_output.built_in_speakers\"",
                "/mix/monitor/device/description \"Built-in speakers\"",
                "ok",
            ]
        );
        assert_eq!(
            h.host.sent_since(from),
            [Command::SetMixDevice {
                mix: MixId::Monitor,
                device: Some(speakers)
            }]
        );
        // A hardware channel's input, as its source panel sends it.
        let synth = h.channel(5);
        let from = h.host.calls.len();
        assert_eq!(
            ask(
                &mut h,
                &controller,
                "set /ch/synth/device/name c920-webcam-mic"
            )
            .last()
            .unwrap(),
            "ok"
        );
        assert_eq!(
            h.host.sent_since(from),
            [Command::SetSource {
                ch: synth,
                source: Source::Hardware(Some(example::device("C920 webcam mic", true)))
            }]
        );
        // An app channel is never made a hardware one, and the host is not asked.
        let from = h.host.calls.len();
        let refused = ask(&mut h, &controller, "set /ch/game/device/name Wave:3");
        assert!(
            refused[0].starts_with(ERROR) && refused[0].contains("Game is an app channel"),
            "{refused:?}"
        );
        assert_eq!(h.host.sent_since(from), Vec::<Command>::new());
    }

    #[test]
    fn a_device_unplugged_reaches_a_client_that_follows_the_list_or_the_strip() {
        let mut h = Harness::new();
        let follower = client(&mut h);
        // The example's four outputs, a name and a description each.
        assert_eq!(ask(&mut h, &follower, "watch /out").len(), 8 + 1);
        assert_eq!(
            ask(&mut h, &follower, "watch /mix/monitor/device/present"),
            ["/mix/monitor/device/present true", "ok"]
        );
        // Monitor's phones are unplugged: the host lists what is left and says so at its
        // next poll.
        let left: Vec<DeviceInfo> = h
            .host
            .devices()
            .iter()
            .filter(|d| d.device.description != example::OUTPUTS[0])
            .cloned()
            .collect();
        h.host.devices = Some(left.clone());
        h.host.news.push(HostEvent::Devices(left));
        h.idle(1);
        assert_eq!(
            said(&follower),
            [
                "event /out/alsa-output-scarlett-2i2-phones gone",
                "event /mix/monitor/device/present false",
            ]
        );
    }
}
