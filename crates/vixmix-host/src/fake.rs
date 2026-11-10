//! [`FakeHost`]: prototype v5's example data and its simulated meters, with no audio, no
//! plugins and no files. The app's headless tests and the look gate run against it, and
//! `cargo run -p vix-mix -- --fake` shows it.
//!
//! The meters are the prototype's `tick`: each channel chases a level `target(style)` picks
//! every 110–490 ms (speech pauses, a game's steady bed, bursts), its left and right sides
//! easing towards it by 0.22 a frame with a little jitter; a send scales it by its fader, a
//! muted send shows nothing, and a mix shows the loudest unmuted send into it scaled by its
//! own fader. A channel with no running app is silent. The randomness is a seeded generator,
//! so a test sees the same signal every run.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use chimaeraproject_vst3_host::{EditorEvent, EditorSize};
use raw_window_handle::RawWindowHandle;
use vixmix_engine::InsertProblem;
use vixmix_model::{
    Applied, ChannelId, Command, DeviceDirection, DeviceInfo, InstanceId, MAX_CHANNELS, Refused,
    Setup, Source, apply, example, taper,
};
use vixmix_plugins::{Catalog, PluginEntry, PluginKind};

use crate::host::{
    AppPresence, BackendState, ChannelLevels, Host, HostError, HostEvent, MeterFrame, StatsFrame,
    Stereo,
};
use crate::undo::removal;

/// One animation frame of the prototype's `requestAnimationFrame` loop.
const FRAME_MS: f64 = 1000.0 / 60.0;

/// How far the simulated worst cycle sits above the simulated load: three points, so the
/// header's DSP bar shows its peak tick just past the fill.
const PEAK_ABOVE: f32 = 0.03;

/// How a channel's simulated signal behaves: the prototype's `style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Speech,
    Steady,
    Game,
    Burst,
    Low,
}

impl Style {
    fn named(name: &str) -> Style {
        match name {
            "speech" => Style::Speech,
            "steady" => Style::Steady,
            "game" => Style::Game,
            "burst" => Style::Burst,
            _ => Style::Low,
        }
    }
}

struct ChannelSim {
    id: ChannelId,
    style: Style,
    left: f64,
    right: f64,
    target: f64,
    next_ms: f64,
}

/// xorshift64*: enough randomness for a meter, the same every run for a given seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let bits = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11;
        bits as f64 / (1u64 << 53) as f64
    }
}

/// The example console with simulated meters; see the module documentation.
pub struct FakeHost {
    setup: Setup,
    catalog: Catalog,
    apps: Vec<AppPresence>,
    devices: Vec<DeviceInfo>,
    sims: Vec<ChannelSim>,
    rng: Rng,
    now_ms: f64,
    last_dsp_ms: f64,
    started: Instant,
    frame: MeterFrame,
    stats: StatsFrame,
    events: Vec<HostEvent>,
}

impl Default for FakeHost {
    fn default() -> FakeHost {
        FakeHost::new()
    }
}

impl FakeHost {
    /// The prototype's example console, its meters already moving.
    pub fn new() -> FakeHost {
        FakeHost::with_seed(0x5eed_0f51_6ea1)
    }

    /// The same, with another seed for the simulated signal.
    pub fn with_seed(seed: u64) -> FakeHost {
        let setup = example::setup();
        let sims = setup
            .channels
            .iter()
            .zip(example::STYLES)
            .map(|(c, style)| ChannelSim::new(c.id, Style::named(style)))
            .collect();
        let apps: Vec<AppPresence> = example::APPS
            .iter()
            .map(|(id, _, running)| AppPresence {
                app: example::app(id),
                running: *running,
            })
            .collect();
        let devices = example::INPUTS
            .iter()
            .map(|d| (d, DeviceDirection::Input, true))
            .chain(
                example::OUTPUTS
                    .iter()
                    .map(|d| (d, DeviceDirection::Output, false)),
            )
            .map(|(d, direction, input)| DeviceInfo {
                device: example::device(d, input),
                direction,
            })
            .collect::<Vec<_>>();
        let plugins = example::PLUGINS
            .iter()
            .map(|(name, vendor, category)| {
                let plugin = example::plugin(name);
                PluginEntry {
                    id: plugin.id,
                    name: (*name).to_string(),
                    vendor: (*vendor).to_string(),
                    kind: PluginKind::from_label(category).unwrap_or(PluginKind::MultiEffect),
                    path: PathBuf::from(&plugin.bundle),
                    bundle: plugin.bundle,
                }
            })
            .collect();
        let catalog = Catalog {
            plugins,
            favorites: example::FAVORITES
                .iter()
                .map(|n| example::plugin(n).id)
                .collect(),
            recent: example::RECENT
                .iter()
                .map(|n| example::plugin(n).id)
                .collect(),
        };
        let mut host = FakeHost {
            setup,
            catalog,
            events: vec![
                HostEvent::Backend(BackendState::Simulated),
                HostEvent::Apps(apps.clone()),
                HostEvent::Devices(devices.clone()),
            ],
            apps,
            devices,
            sims,
            rng: Rng(seed | 1),
            now_ms: 0.0,
            last_dsp_ms: f64::NEG_INFINITY,
            started: Instant::now(),
            frame: MeterFrame::default(),
            stats: StatsFrame {
                rate: 48_000,
                quantum: 256,
                dsp_load: 0.07,
                dsp_peak: 0.07 + PEAK_ABOVE,
                xruns: 0,
            },
        };
        // The prototype's reduced-motion start: one settled frame, so the first paint is not
        // a row of empty meters.
        host.tick(true);
        host
    }

    /// Moves the simulation `ms` milliseconds on, one animation frame at a time. `poll` does
    /// this with the wall clock; a test calls it to step deterministically.
    pub fn advance(&mut self, ms: f64) {
        let target = self.now_ms + ms;
        while self.now_ms + FRAME_MS <= target {
            self.now_ms += FRAME_MS;
            self.tick(false);
        }
    }

    /// The prototype's `tick(t, settle)`.
    fn tick(&mut self, settle: bool) {
        let now = self.now_ms;
        let mut sums = [[0.0f64; 2]; 2]; // [monitor, master][left, right]
        let mut frame = MeterFrame::default();
        for (index, channel) in self.setup.channels.iter().enumerate().take(MAX_CHANNELS) {
            let at = match self.sims.iter().position(|s| s.id == channel.id) {
                Some(at) => at,
                None => {
                    // A channel added since start: the prototype's new-channel styles.
                    let style = match channel.source {
                        Source::Hardware(_) => Style::Burst,
                        Source::Apps(_) => Style::Low,
                    };
                    self.sims.push(ChannelSim::new(channel.id, style));
                    self.sims.len() - 1
                }
            };
            let rng = &mut self.rng;
            let sim = &mut self.sims[at];
            if now > sim.next_ms {
                sim.target = target(sim.style, rng);
                sim.next_ms = now + 110.0 + rng.next() * 380.0;
            }
            let live = match &channel.source {
                Source::Hardware(_) => true,
                Source::Apps(apps) => apps
                    .iter()
                    .any(|a| self.apps.iter().any(|p| p.running && p.app.matches(a))),
            };
            let goal = if live { sim.target } else { 0.0 };
            let k = if settle { 1.0 } else { 0.22 };
            sim.left += (goal * (0.92 + rng.next() * 0.12) - sim.left) * k;
            sim.right += (goal * (0.84 + rng.next() * 0.14) - sim.right) * k;
            let (left, right) = (audible(sim.left), audible(sim.right));
            let mut levels = ChannelLevels::default();
            for (mix, send, out) in [
                (0, &channel.monitor, &mut levels.monitor),
                (1, &channel.master, &mut levels.master),
            ] {
                if send.mute {
                    continue;
                }
                let gain = f64::from(taper::position_to_gain(send.position));
                *out = Stereo {
                    left: (left * gain) as f32,
                    right: (right * gain) as f32,
                };
                sums[mix][0] = sums[mix][0].max(left * gain);
                sums[mix][1] = sums[mix][1].max(right * gain);
            }
            frame.channels[index] = levels;
        }
        let monitor_sum = if self.setup.preview_master {
            sums[1]
        } else {
            sums[0]
        };
        for (sum, level, out) in [
            (monitor_sum, &self.setup.monitor.level, &mut frame.monitor),
            (sums[1], &self.setup.master.level, &mut frame.master),
        ] {
            let gain = if level.mute {
                0.0
            } else {
                f64::from(taper::position_to_gain(level.position))
            };
            *out = Stereo {
                left: (sum[0] * gain) as f32,
                right: (sum[1] * gain) as f32,
            };
        }
        self.frame = frame;
        if now - self.last_dsp_ms > 900.0 {
            self.last_dsp_ms = now;
            self.stats.dsp_load = (6.0 + (self.rng.next() * 3.0).round()) as f32 / 100.0;
            self.stats.dsp_peak = self.stats.dsp_load + PEAK_ABOVE;
        }
    }
}

impl ChannelSim {
    fn new(id: ChannelId, style: Style) -> ChannelSim {
        ChannelSim {
            id,
            style,
            left: 0.0,
            right: 0.0,
            target: 0.2,
            next_ms: 0.0,
        }
    }
}

/// The prototype's `target(style)`: the level a channel heads for next.
fn target(style: Style, rng: &mut Rng) -> f64 {
    let r = rng.next();
    match style {
        Style::Speech if r < 0.3 => 0.01,
        Style::Speech => 0.25 + rng.next() * 0.45,
        Style::Steady => 0.3 + rng.next() * 0.25,
        Style::Game => 0.15 + rng.next() * 0.3,
        Style::Burst if r < 0.5 => 0.005,
        Style::Burst => 0.15 + rng.next() * 0.35,
        Style::Low if r < 0.6 => 0.005,
        Style::Low => 0.06 + rng.next() * 0.12,
    }
}

/// The prototype's `amp2db` floor: at or below 0.0001 the meter reads silence.
fn audible(amplitude: f64) -> f64 {
    if amplitude <= 0.0001 { 0.0 } else { amplitude }
}

impl Host for FakeHost {
    fn setup(&self) -> &Setup {
        &self.setup
    }

    fn apply(&mut self, cmd: Command) -> Result<Applied, Refused> {
        // The same undo the real host hands out, so the window's toast works against either.
        let gone = removal(&self.setup, &cmd);
        let added = match &cmd {
            Command::AddInsert { plugin, .. } => Some(plugin.id.clone()),
            _ => None,
        };
        let applied = apply(&mut self.setup, cmd)?;
        if let Some(gone) = gone.filter(|_| applied.changed()) {
            self.events.push(HostEvent::Removed {
                message: gone.message,
                undo: gone.undo,
            });
        }
        if let Some(id) = added {
            self.catalog.used(&id);
        }
        if applied.changed() {
            self.events.push(HostEvent::SetupChanged);
        }
        Ok(applied)
    }

    fn poll(&mut self) -> Vec<HostEvent> {
        let elapsed = self.started.elapsed().as_secs_f64() * 1000.0;
        self.advance(elapsed - self.now_ms);
        std::mem::take(&mut self.events)
    }

    fn meters(&self) -> MeterFrame {
        self.frame
    }

    fn stats(&self) -> StatsFrame {
        self.stats
    }

    fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    fn apps(&self) -> &[AppPresence] {
        &self.apps
    }

    fn devices(&self) -> &[DeviceInfo] {
        &self.devices
    }

    fn open_editor(
        &mut self,
        _instance: InstanceId,
        _parent: RawWindowHandle,
    ) -> Result<EditorSize, HostError> {
        Err(HostError::Simulated)
    }

    fn tick_editors(&mut self) -> Vec<(InstanceId, EditorEvent)> {
        Vec::new()
    }

    fn resize_editor(&mut self, _instance: InstanceId, _size: EditorSize) {}

    fn close_editor(&mut self, _instance: InstanceId) {}

    fn save_now(&mut self) {}

    fn problems(&self) -> &[InsertProblem] {
        &[]
    }

    fn set_favorite(&mut self, plugin_id: &str, favorite: bool) {
        self.catalog.set_favorite(plugin_id, favorite);
    }

    fn rescan_plugins(&mut self) {
        self.events.push(HostEvent::PluginsRescanned);
    }

    /// Never: its meters are live, so the window draws (and polls) at its frame rate.
    fn next_poll(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::MixId;

    #[test]
    fn the_first_poll_reports_a_simulated_backend_with_the_examples_apps_and_devices() {
        let mut host = FakeHost::new();
        let events = host.poll();
        assert!(events.contains(&HostEvent::Backend(BackendState::Simulated)));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, HostEvent::Apps(a) if a.len() == 13))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, HostEvent::Devices(d) if d.len() == 8))
        );
        assert_eq!(host.setup().channels.len(), 6);
        assert_eq!(host.catalog().plugins.len(), 42);
        assert!(BackendState::Simulated.live());
        assert_eq!(BackendState::Simulated.banner(), None);
    }

    #[test]
    fn meters_move_mutes_silence_them_and_the_same_seed_gives_the_same_signal() {
        let mut a = FakeHost::with_seed(7);
        let mut b = FakeHost::with_seed(7);
        a.advance(2_000.0);
        b.advance(2_000.0);
        assert_eq!(a.meters(), b.meters());
        let frame = a.meters();
        // Music's Master send is muted in the example; its Monitor send is not.
        assert_eq!(frame.channels[2].master, Stereo::default());
        assert!(frame.channels.iter().any(|c| c.monitor.left > 0.0));
        assert!(frame.master.left > 0.0);
        // Channels past the sixth are empty.
        assert_eq!(frame.channels[7], ChannelLevels::default());

        let _ = a.apply(Command::SetMixMute {
            mix: MixId::Master,
            mute: true,
        });
        a.advance(100.0);
        assert_eq!(a.meters().master, Stereo::default());
    }

    #[test]
    fn a_removal_hands_out_the_undo_that_puts_it_back() {
        let mut host = FakeHost::with_seed(5);
        let before = host.setup().clone();
        let ch = before.channels[1].id;
        let _ = host.poll();
        let _ = host.apply(Command::RemoveChannel(ch)).unwrap();
        let undo = host
            .poll()
            .into_iter()
            .find_map(|e| match e {
                HostEvent::Removed { message, undo } => {
                    assert_eq!(
                        message,
                        format!("Removed channel {}", before.channels[1].name)
                    );
                    Some(undo)
                }
                _ => None,
            })
            .expect("the removal's undo");
        let _ = host.apply(undo).unwrap();
        assert_eq!(host.setup(), &before);
        let id = host.catalog().plugins[0].id.clone();
        host.set_favorite(&id, true);
        assert!(host.catalog().favorites.contains(&id));
        host.rescan_plugins();
        assert!(host.poll().contains(&HostEvent::PluginsRescanned));
        assert!(host.problems().is_empty() && host.next_poll().is_none());
    }

    #[test]
    fn the_dsp_load_reads_6_to_9_percent_with_its_peak_three_points_above() {
        let mut host = FakeHost::with_seed(11);
        for _ in 0..6 {
            let stats = host.stats();
            assert!((0.06..=0.09).contains(&stats.dsp_load), "{stats:?}");
            assert!(
                (stats.dsp_peak - stats.dsp_load - PEAK_ABOVE).abs() < 1e-6,
                "{stats:?}"
            );
            host.advance(1_000.0);
        }
    }

    #[test]
    fn previewing_master_shows_the_master_sum_on_monitor() {
        let mut host = FakeHost::with_seed(3);
        let _ = host.apply(Command::SetPreview(true)).unwrap();
        host.advance(500.0);
        let frame = host.meters();
        // Both mixes sit at unity in the example, so the two meters read the same sum.
        assert_eq!(frame.monitor, frame.master);
    }
}
