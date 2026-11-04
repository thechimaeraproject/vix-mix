//! [`RealHost`]: the setup from `setup.ron`, the plugin instances it names, the engine
//! prepared from it, and the backend — PipeWire on Linux once handoff 03 builds it, the null
//! backend everywhere until then — wired together on the UI thread.
//!
//! What it does today (handoff 00): boots, loads the setup and the plugin cache, creates the
//! instances the catalog can find, prepares the engine, pumps the null backend one quantum a
//! poll so meters and stats are live, applies commands (creating and retiring instances,
//! preparing again), and saves on request. Handoff 06 makes it the whole orchestrator:
//! routing, the debounced autosave, shutdown order.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chimaeraproject_vst3_host::{EditorEvent, EditorSize, HostContext};
use raw_window_handle::RawWindowHandle;
use vixmix_engine::{Engine, EngineConfig, MAX_QUANTUM, NullBackend};
use vixmix_model::{
    Applied, Command, DeviceInfo, InstanceId, Refused, Setup, apply, persist, snapshot,
};
use vixmix_plugins::{Catalog, EditorSession, InstanceEvent, Instances, Rack};

use crate::host::{
    AppPresence, BackendState, ChannelLevels, Host, HostError, HostEvent, MeterFrame, StatsFrame,
    Stereo,
};

/// The quantum the null backend pumps per poll.
const NULL_QUANTUM: usize = 256;

/// Where [`RealHost::boot`] reads and writes.
#[derive(Clone, Debug, Default)]
pub struct BootOptions {
    /// The directory holding `setup.ron` and `plugins.ron`; `None` resolves the user's
    /// configuration directory for `vix-mix` with the kit's profile rule.
    pub config_dir: Option<PathBuf>,
    /// `cfg!(debug_assertions)` of the binary: a debug build keeps its own profile.
    pub debug_build: bool,
}

/// The real thing; see the module documentation.
pub struct RealHost {
    dir: PathBuf,
    setup: Setup,
    /// The setup file existed but could not be used: move it aside before the first save.
    unreadable: bool,
    catalog: Catalog,
    engine: Arc<Engine>,
    rack: Arc<Rack>,
    instances: Instances,
    editors: Vec<EditorSession>,
    null: NullBackend,
    backend: BackendState,
    apps: Vec<AppPresence>,
    devices: Vec<DeviceInfo>,
    events: Vec<HostEvent>,
}

impl RealHost {
    /// Loads the setup and the plugin cache, creates the instances, prepares the engine and
    /// starts the backend.
    pub fn boot(options: BootOptions) -> Result<RealHost, HostError> {
        let dir = match options.config_dir {
            Some(dir) => dir,
            None => {
                persist::profile(options.debug_build)
                    .ok_or(HostError::NoConfigDir)?
                    .dir
            }
        };
        let mut events = Vec::new();
        let (setup, problem) = persist::load(&dir).into_parts();
        let unreadable = problem.is_some();
        if let Some(problem) = problem {
            events.push(HostEvent::Toast(problem.to_string()));
        }
        let (catalog, problem) = Catalog::load(&dir).into_parts();
        if let Some(problem) = problem {
            events.push(HostEvent::Toast(problem.to_string()));
        }

        let engine = Arc::new(Engine::new(EngineConfig::new(48_000, MAX_QUANTUM, 2)));
        let rack = Rack::new();
        let instances = Instances::new(HostContext::new("Vix Mix"), Arc::clone(&rack));
        let backend = start_backend(&engine);
        events.push(HostEvent::Backend(backend.clone()));
        let null = NullBackend::new(Arc::clone(&engine), NULL_QUANTUM);

        let mut host = RealHost {
            dir,
            setup,
            unreadable,
            catalog,
            engine,
            rack,
            instances,
            editors: Vec::new(),
            null,
            backend,
            apps: Vec::new(),
            devices: Vec::new(),
            events,
        };
        let inserts: Vec<_> = host
            .setup
            .inserts()
            .map(|i| (i.instance, i.plugin.clone(), i.state.clone()))
            .collect();
        for (instance, plugin, state) in inserts {
            host.create(instance, &plugin, state.as_deref());
        }
        host.prepare();
        Ok(host)
    }

    /// Where the setup is read and written.
    pub fn config_dir(&self) -> &Path {
        &self.dir
    }

    /// The backend's state now.
    pub fn backend(&self) -> &BackendState {
        &self.backend
    }

    /// The engine, for a test that wants its counters.
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    fn create(
        &mut self,
        instance: InstanceId,
        plugin: &vixmix_model::PluginRef,
        state: Option<&[u8]>,
    ) {
        if let Err(error) = self
            .instances
            .create(instance, plugin, &self.catalog, state)
        {
            log::warn!("{error}");
            self.events.push(HostEvent::Toast(error.to_string()));
        }
    }

    fn prepare(&self) {
        self.engine.prepare(&snapshot(&self.setup), &*self.rack);
    }
}

/// Linux: PipeWire, which answers "not built" until handoff 03. Elsewhere: nothing.
fn start_backend(engine: &Arc<Engine>) -> BackendState {
    #[cfg(target_os = "linux")]
    {
        let (events, _receiver) = std::sync::mpsc::sync_channel(256);
        match vixmix_pipewire::Backend::connect(
            vixmix_pipewire::BackendConfig::default(),
            Arc::clone(engine),
            events,
        ) {
            // Handoff 06 keeps the backend and its receiver; nothing can connect before 03.
            Ok(backend) => {
                backend.disconnect();
                BackendState::Connecting
            }
            Err(error) => BackendState::Absent {
                reason: format!("No audio backend: {error}"),
            },
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = engine;
        BackendState::Absent {
            reason: BackendState::NO_BACKEND.to_string(),
        }
    }
}

impl Host for RealHost {
    fn setup(&self) -> &Setup {
        &self.setup
    }

    fn apply(&mut self, cmd: Command) -> Result<Applied, Refused> {
        let applied = apply(&mut self.setup, cmd)?;
        for instance in &applied.removed {
            self.instances.retire(*instance);
        }
        for instance in &applied.created {
            let found = self
                .setup
                .inserts()
                .find(|i| i.instance == *instance)
                .map(|i| (i.plugin.clone(), i.state.clone()));
            if let Some((plugin, state)) = found {
                self.create(*instance, &plugin, state.as_deref());
            }
        }
        if applied.engine || !applied.created.is_empty() || !applied.removed.is_empty() {
            self.prepare();
        }
        if applied.changed() {
            self.events.push(HostEvent::SetupChanged);
        }
        Ok(applied)
    }

    fn poll(&mut self) -> Vec<HostEvent> {
        if matches!(self.backend, BackendState::Absent { .. }) {
            self.null.pump(NULL_QUANTUM);
        }
        self.engine.reap();
        self.instances.reap();
        let mut prepare = false;
        for event in self.instances.tick() {
            match event {
                InstanceEvent::Dirty(_) => self.events.push(HostEvent::SetupChanged),
                InstanceEvent::IoChanged(_) => prepare = true,
                // Reaching the processor is handoffs 02 and 04's path.
                InstanceEvent::Edit { .. } => {}
            }
        }
        if prepare {
            self.prepare();
        }
        std::mem::take(&mut self.events)
    }

    fn meters(&self) -> MeterFrame {
        let bank = self.engine.meters();
        let stereo = |(left, right): (f32, f32)| Stereo { left, right };
        let mut frame = MeterFrame::default();
        for (index, levels) in frame
            .channels
            .iter_mut()
            .take(self.setup.channels.len())
            .enumerate()
        {
            let meters = bank.channel(index);
            *levels = ChannelLevels {
                monitor: stereo(meters.monitor.take()),
                master: stereo(meters.master.take()),
            };
        }
        frame.monitor = stereo(bank.monitor().take());
        frame.master = stereo(bank.master().take());
        frame
    }

    fn stats(&self) -> StatsFrame {
        use std::sync::atomic::Ordering::Relaxed;
        let stats = self.engine.stats();
        StatsFrame {
            rate: stats.rate.load(Relaxed),
            quantum: stats.quantum.load(Relaxed),
            dsp_load: stats.dsp_load_permille.load(Relaxed) as f32 / 1000.0,
            xruns: stats.xruns.load(Relaxed),
        }
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
        instance: InstanceId,
        parent: RawWindowHandle,
    ) -> Result<EditorSize, HostError> {
        self.close_editor(instance);
        let mut session = self
            .instances
            .editor(instance)?
            .ok_or(HostError::NoEditor(instance))?;
        let size = session.open(parent)?;
        self.editors.push(session);
        Ok(size)
    }

    fn tick_editors(&mut self) -> Vec<(InstanceId, EditorEvent)> {
        let mut out = Vec::new();
        for session in &mut self.editors {
            if let Some(run_loop) = session.run_loop() {
                run_loop.poll();
            }
            out.extend(
                session
                    .poll_events()
                    .into_iter()
                    .map(|e| (session.instance(), e)),
            );
        }
        out
    }

    fn close_editor(&mut self, instance: InstanceId) {
        if let Some(at) = self.editors.iter().position(|s| s.instance() == instance) {
            let mut session = self.editors.remove(at);
            session.close();
        }
    }

    fn save_now(&mut self) {
        let live: Vec<InstanceId> = self
            .setup
            .inserts()
            .map(|i| i.instance)
            .filter(|i| self.instances.is_live(*i))
            .collect();
        for instance in live {
            match self.instances.save_state(instance) {
                Ok(state) => {
                    let _ = apply(
                        &mut self.setup,
                        Command::SetInsertState {
                            instance,
                            state: Some(state),
                        },
                    );
                }
                Err(error) => self.events.push(HostEvent::Toast(error.to_string())),
            }
        }
        if let Err(error) = std::fs::create_dir_all(&self.dir) {
            self.events.push(HostEvent::Toast(format!(
                "{} could not be created ({error}): check the parent directory is writable; the setup was not saved",
                self.dir.display()
            )));
            return;
        }
        if self.unreadable {
            // Never save over a file the user may still want: keep it beside the new one.
            let path = persist::path(&self.dir);
            let aside = path.with_extension("ron.unreadable");
            match std::fs::rename(&path, &aside) {
                Ok(()) => self.events.push(HostEvent::Toast(format!(
                    "the setup that could not be read is kept as {}",
                    aside.display()
                ))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    self.events.push(HostEvent::Toast(format!(
                        "{} could not be moved aside ({error}): move it by hand; the setup was not saved over it",
                        path.display()
                    )));
                    return;
                }
            }
            self.unreadable = false;
        }
        if let Err(error) = persist::save(&self.dir, &self.setup) {
            self.events.push(HostEvent::Toast(error.to_string()));
        }
    }
}

impl Drop for RealHost {
    fn drop(&mut self) {
        // Every editor closes before its window and its instance go.
        for session in &mut self.editors {
            session.close();
        }
        self.editors.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_model::MixId;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vixmix-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn boot(dir: &Path) -> RealHost {
        RealHost::boot(BootOptions {
            config_dir: Some(dir.to_path_buf()),
            debug_build: true,
        })
        .unwrap()
    }

    #[test]
    fn it_boots_on_the_null_backend_and_says_so_first() {
        let dir = scratch("boot");
        let mut host = boot(&dir);
        let events = host.poll();
        let banner = events
            .iter()
            .find_map(|e| match e {
                HostEvent::Backend(state) => state.banner().map(str::to_string),
                _ => None,
            })
            .expect("the first poll reports the backend");
        if cfg!(target_os = "linux") {
            assert!(banner.contains("not built yet"), "{banner}");
        } else {
            assert_eq!(banner, BackendState::NO_BACKEND);
        }
        assert_eq!(host.setup(), &Setup::default());
        // Each poll pumps one quantum through the engine.
        let _ = host.poll();
        let stats = host.stats();
        assert_eq!(stats.rate, 48_000);
        assert_eq!(stats.quantum, NULL_QUANTUM as u32);
        assert!(
            host.engine()
                .stats()
                .frames
                .load(std::sync::atomic::Ordering::Relaxed)
                >= 2 * NULL_QUANTUM as u64
        );
        assert!(!dir.exists(), "booting writes nothing");
    }

    #[test]
    fn a_command_changes_the_setup_and_a_save_restores_it_at_the_next_boot() {
        let dir = scratch("save");
        let mut host = boot(&dir);
        let ch = host.setup().channels[0].id;
        let _ = host
            .apply(Command::SetMute {
                ch,
                mix: MixId::Master,
                mute: true,
            })
            .unwrap();
        assert!(host.poll().contains(&HostEvent::SetupChanged));
        host.save_now();
        let saved = host.setup().clone();
        drop(host);
        let host = boot(&dir);
        assert_eq!(host.setup(), &saved);
        assert!(host.setup().channels[0].master.mute);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_setup_is_kept_aside_rather_than_saved_over() {
        let dir = scratch("unreadable");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(persist::path(&dir), "not a setup").unwrap();
        let mut host = boot(&dir);
        let events = host.poll();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, HostEvent::Toast(t) if t.contains("setup.ron"))),
            "{events:?}"
        );
        host.save_now();
        assert_eq!(
            std::fs::read_to_string(dir.join("setup.ron.unreadable")).unwrap(),
            "not a setup"
        );
        assert!(persist::path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
