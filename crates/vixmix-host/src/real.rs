//! [`RealHost`]: the setup from `setup.ron`, the plugin instances it names, the engine
//! prepared from it, and the backend — PipeWire on Linux, the null backend elsewhere — wired
//! together on the UI thread, in the order handoff 06 sets: boot, routing, acting on each
//! command's `Applied`, the debounced autosave, the instances' lifecycle and shutdown.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chimaeraproject_vst3_host::{EditorEvent, EditorSize, HostContext};
use raw_window_handle::RawWindowHandle;
use vixmix_engine::{Engine, EngineConfig, InsertProblem, Lender, MASTER, MAX_QUANTUM, MONITOR};
use vixmix_model::{
    AppRef, Applied, Command, DeviceInfo, Insert, InstanceId, Mix, MixId, Refused, Setup, Strip,
    apply, persist, snapshot,
};
use vixmix_plugins::{
    Catalog, EditorSession, InstanceEvent, Instances, Rack, ScanCache, ScanOutcome, search_dirs,
};

use crate::apps::{presence, remember};
use crate::audio::{Audio, AudioBackend, Heard, POLL_INTERVAL};
use crate::autosave::Autosave;
use crate::host::{
    AppPresence, BackendState, ChannelLevels, Host, HostError, HostEvent, MeterFrame, StatsFrame,
    Stereo, UNDO_TOAST,
};
use crate::undo::removal;

/// How long a removed insert's instance stays live: the toast, and a margin for the window's
/// own clock.
const KEEP_REMOVED: Duration = Duration::from_secs(UNDO_TOAST.as_secs() + 1);

/// The most removed instances kept for an undo at once; past it the oldest is let go early
/// (its undo then makes it again from the state it was removed with).
const MAX_KEPT: usize = 64;

/// How many scan failures a toast names before it counts the rest.
const FAILURES_NAMED: usize = 2;

/// Where [`RealHost::boot`] reads and writes, and what it plays through.
#[derive(Clone, Debug, Default)]
pub struct BootOptions {
    /// The directory holding `setup.ron` and `plugins.ron`; `None` resolves the user's
    /// configuration directory for `vix-mix` with the kit's profile rule.
    pub config_dir: Option<PathBuf>,
    /// `cfg!(debug_assertions)` of the binary: a debug build keeps its own profile.
    pub debug_build: bool,
    /// The backend. The default is the null backend, so a test never touches the desktop's
    /// audio; the app asks for [`AudioBackend::APP`].
    pub audio: AudioBackend,
    /// Where to look for plugins: `None` is the SDK's standard places and the preferences'
    /// extra directories; `Some` is only these (a test's).
    pub plugin_dirs: Option<Vec<PathBuf>>,
}

/// A removed insert whose instance stays live until its undo toast dies.
struct Kept {
    insert: Insert,
    until: Instant,
}

/// The real thing; see the module documentation.
pub struct RealHost {
    dir: PathBuf,
    setup: Setup,
    /// The setup file existed but could not be used: move it aside before the first save.
    unreadable: bool,
    vst3: HostContext,
    plugin_dirs: Option<Vec<PathBuf>>,
    catalog: Catalog,
    cache: ScanCache,
    engine: Arc<Engine>,
    rack: Arc<Rack>,
    instances: Instances,
    editors: Vec<EditorSession>,
    audio: Audio,
    backend: BackendState,
    apps: Vec<AppPresence>,
    /// Apps that played since start, for the source panel's "Seen before".
    seen: Vec<AppRef>,
    problems: Vec<InsertProblem>,
    kept: Vec<Kept>,
    autosave: Autosave,
    events: Vec<HostEvent>,
}

impl RealHost {
    /// Loads the setup and the plugin cache, rescans the plugins, creates the instances,
    /// prepares the engine and starts the backend with the setup's graph.
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
        let vst3 = HostContext::new("Vix Mix");
        let (catalog, problem) = Catalog::load(&dir).into_parts();
        if let Some(problem) = problem {
            events.push(HostEvent::Toast(problem.to_string()));
        }
        let cache = ScanCache::load(&dir);

        let engine = Arc::new(Engine::new(EngineConfig::new(48_000, MAX_QUANTUM, 2)));
        let rack = Rack::new();
        let instances = Instances::new(vst3.clone(), Arc::clone(&rack));
        let audio = Audio::start(AudioBackend::Null, &engine, &setup);
        let mut host = RealHost {
            dir,
            setup,
            unreadable,
            vst3,
            plugin_dirs: options.plugin_dirs,
            catalog,
            cache,
            engine,
            rack,
            instances,
            editors: Vec::new(),
            audio,
            backend: BackendState::Connecting,
            apps: Vec::new(),
            seen: Vec::new(),
            problems: Vec::new(),
            kept: Vec::new(),
            autosave: Autosave::default(),
            events,
        };
        host.scan(false);
        host.sync_instances();
        host.prepare();
        // The engine has its graph before the first tick can ask for one.
        if options.audio != AudioBackend::Null {
            host.audio = Audio::start(options.audio, &host.engine, &host.setup);
        }
        host.backend = host.audio.state();
        host.events.push(HostEvent::Backend(host.backend.clone()));
        host.refresh_apps();
        host.take_notices();
        host.refresh_problems();
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

    /// Whether `instance` is a live plugin instance: a test's view of the lifecycle.
    pub fn is_live(&self, instance: InstanceId) -> bool {
        self.instances.is_live(instance)
    }

    /// On the null backend, the input of the channel at `index` in the console (left and
    /// right), which every later poll plays until it is written again: how a test feeds a
    /// signal through the mixer with no audio system. `None` on a real backend.
    pub fn null_input(&mut self, index: usize) -> Option<(&mut [f32], &mut [f32])> {
        self.audio.null_input(index)
    }

    /// Moves parameter `param` of `instance` to `value` (normalized, 0 to 1) as the mixer's
    /// own control would for a plugin without an editor: the plugin's controller shows it,
    /// its processor plays it from its next block, and the setup is saved with the plugin's
    /// new state once the debounce runs out (or at quit). A test changes a plugin's state
    /// while it plays this way, as an edit in its editor would.
    pub fn set_param(
        &mut self,
        instance: InstanceId,
        param: u32,
        value: f64,
    ) -> Result<(), HostError> {
        self.instances.set_param(instance, param, value)?;
        self.autosave.touch(Instant::now());
        self.take_notices();
        Ok(())
    }

    /// The directories a scan walks.
    fn scan_dirs(&self) -> Vec<PathBuf> {
        match &self.plugin_dirs {
            Some(dirs) => dirs.clone(),
            None => search_dirs(&self.setup.prefs.plugin_dirs),
        }
    }

    /// Describes every new or changed bundle (and, after `forget_failures`, every one that
    /// failed), and keeps the outcome in `plugins.ron` when it learned anything.
    fn scan(&mut self, forget_failures: bool) {
        if forget_failures {
            self.cache.forget_failures();
        }
        let dirs = self.scan_dirs();
        let outcome = self.catalog.rescan(&dirs, &self.vst3, &self.cache);
        log::info!(
            "plugins: {} found, {} bundles described, {} unchanged, {} failed",
            outcome.catalog.plugins.len(),
            outcome.described,
            outcome.reused,
            outcome.failures.len()
        );
        if let Some(toast) = failures(&outcome.failures) {
            self.events.push(HostEvent::Toast(toast));
        }
        if forget_failures || outcome.described > 0 || outcome.cache.len() != self.cache.len() {
            self.write_plugins(&outcome);
        }
        self.catalog = outcome.catalog;
        self.cache = outcome.cache;
    }

    fn write_plugins(&mut self, outcome: &ScanOutcome) {
        let written = std::fs::create_dir_all(&self.dir)
            .map_err(|e| e.to_string())
            .and_then(|()| outcome.save(&self.dir).map_err(|e| e.to_string()));
        if let Err(error) = written {
            self.events.push(HostEvent::Toast(format!(
                "the plugin list could not be kept in {} ({error}): check the directory is writable; the next start scans again",
                self.dir.display()
            )));
        }
    }

    fn write_catalog(&mut self) {
        let written = std::fs::create_dir_all(&self.dir)
            .map_err(|e| e.to_string())
            .and_then(|()| self.catalog.save(&self.dir).map_err(|e| e.to_string()));
        if let Err(error) = written {
            self.events.push(HostEvent::Toast(format!(
                "favourites and recent plugins could not be kept in {} ({error}): check the directory is writable",
                self.dir.display()
            )));
        }
    }

    /// Makes the live instances match the setup's inserts and the removed ones kept for an
    /// undo: retires the rest, creates what is missing with its stored state, reports a
    /// plugin that cannot be made once (its insert passes audio through).
    fn sync_instances(&mut self) {
        let kept = self.kept.iter().map(|k| &k.insert);
        self.instances
            .sync(self.setup.inserts().chain(kept), &self.catalog);
        let gone: Vec<InstanceId> = self
            .editors
            .iter()
            .map(EditorSession::instance)
            .filter(|i| !self.instances.is_live(*i))
            .collect();
        for instance in gone {
            self.close_editor(instance);
            self.events.push(HostEvent::EditorClosed(instance));
        }
    }

    fn prepare(&self) {
        self.engine.prepare(&snapshot(&self.setup), &*self.rack);
    }

    /// The current state of each live instance among `instances`.
    fn fresh_states(&mut self, instances: &[InstanceId]) -> Vec<(InstanceId, Vec<u8>)> {
        let mut out = Vec::new();
        for instance in instances {
            if !self.instances.is_live(*instance) {
                continue;
            }
            match self.instances.save_state(*instance) {
                Ok(state) => out.push((*instance, state)),
                Err(error) => self.events.push(HostEvent::Toast(error.to_string())),
            }
        }
        out
    }

    /// Puts every live instance's state into the setup; whether any changed.
    fn capture_all(&mut self) -> bool {
        let live: Vec<InstanceId> = self.setup.inserts().map(|i| i.instance).collect();
        let fresh = self.fresh_states(&live);
        store_states(&mut self.setup, fresh)
    }

    /// Writes the setup, moving an unreadable file aside first.
    fn write(&mut self) {
        self.autosave.saved();
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

    /// Keeps a removed insert's instance live until its undo toast dies.
    fn keep(&mut self, insert: Insert, now: Instant) {
        if self.kept.len() == MAX_KEPT {
            self.kept.remove(0);
        }
        self.kept.push(Kept {
            insert,
            until: now + KEEP_REMOVED,
        });
    }

    fn refresh_apps(&mut self) {
        for stream in self.audio.streams() {
            remember(&mut self.seen, &stream.app);
        }
        let apps = presence(
            self.audio.streams().iter().map(|s| &s.app),
            &self.setup,
            &self.seen,
        );
        if apps != self.apps {
            self.apps = apps.clone();
            self.events.push(HostEvent::Apps(apps));
        }
    }

    fn refresh_problems(&mut self) {
        let problems = self.engine.problems();
        if problems != self.problems {
            self.problems = problems.clone();
            self.events.push(HostEvent::Problems(problems));
        }
    }

    fn take_notices(&mut self) {
        for notice in self.instances.take_notices() {
            self.events.push(HostEvent::Toast(notice));
        }
    }

    fn heard(&mut self, heard: Heard) {
        for toast in heard.toasts {
            self.events.push(HostEvent::Toast(toast));
        }
        if heard.devices {
            self.events
                .push(HostEvent::Devices(self.audio.devices().to_vec()));
        }
        if heard.apps {
            self.refresh_apps();
        }
    }
}

/// Stores each state in the setup; whether any differed from what it held.
fn store_states(setup: &mut Setup, states: Vec<(InstanceId, Vec<u8>)>) -> bool {
    let mut changed = false;
    for (instance, state) in states {
        let cmd = Command::SetInsertState {
            instance,
            state: Some(state),
        };
        changed |= apply(setup, cmd).is_ok_and(|applied| applied.save);
    }
    changed
}

/// The instances `cmd` copies or removes: their live state goes with them.
fn carried(setup: &Setup, cmd: &Command) -> Vec<InstanceId> {
    match cmd {
        Command::RemoveInsert { instance } => vec![*instance],
        Command::RemoveChannel(ch) => setup
            .channel(*ch)
            .map(|c| c.chain.iter().map(|i| i.instance).collect())
            .unwrap_or_default(),
        Command::MoveInsert {
            from, copy: true, ..
        } => {
            let chain = match from.strip {
                Strip::Channel(ch) => setup.channel(ch).map(|c| &c.chain),
                Strip::Mix(mix) => Some(&setup.mix(mix).chain),
            };
            chain
                .and_then(|c| c.get(from.index))
                .map(|i| vec![i.instance])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// One toast for a scan's failures: the first few with their fix, and how many more.
fn failures(failures: &[String]) -> Option<String> {
    for failure in failures {
        log::warn!("plugin scan: {failure}");
    }
    let (named, rest) = failures.split_at(failures.len().min(FAILURES_NAMED));
    if named.is_empty() {
        return None;
    }
    let mut toast = named.join("; ");
    if !rest.is_empty() {
        toast.push_str(&format!(
            "; and {} more plugin bundles could not be read (the log names them)",
            rest.len()
        ));
    }
    Some(toast)
}

/// A setup with nothing in it: preparing it lets every lent instance go home.
fn nothing() -> Setup {
    Setup {
        channels: Vec::new(),
        monitor: Mix::new(MixId::Monitor),
        master: Mix::new(MixId::Master),
        ..Setup::default()
    }
}

impl Host for RealHost {
    fn setup(&self) -> &Setup {
        &self.setup
    }

    fn apply(&mut self, cmd: Command) -> Result<Applied, Refused> {
        let now = Instant::now();
        // A removed or copied plugin carries its live state, so an undo or the copy sounds
        // as the plugin did; worked out on a copy of the setup, so a refusal changes nothing.
        let carried = carried(&self.setup, &cmd);
        let fresh = self.fresh_states(&carried);
        let added = match &cmd {
            Command::AddInsert { plugin, .. } => Some(plugin.id.clone()),
            _ => None,
        };
        let (applied, gone) = if fresh.is_empty() {
            let gone = removal(&self.setup, &cmd);
            (apply(&mut self.setup, cmd)?, gone)
        } else {
            let mut next = self.setup.clone();
            store_states(&mut next, fresh);
            let gone = removal(&next, &cmd);
            let applied = apply(&mut next, cmd)?;
            self.setup = next;
            (applied, gone)
        };

        if let Some(gone) = gone.filter(|_| applied.changed()) {
            for insert in gone.inserts {
                self.keep(insert, now);
            }
            self.events.push(HostEvent::Removed {
                message: gone.message,
                undo: gone.undo,
            });
        }
        // An undo brings back the instance kept for it.
        self.kept
            .retain(|k| !applied.created.contains(&k.insert.instance));
        if let Some(id) = added {
            self.catalog.used(&id);
            self.write_catalog();
        }
        // A removed instance stays live (kept); its editor closes with its slot.
        for instance in &applied.removed {
            if self.editors.iter().any(|s| s.instance() == *instance) {
                self.close_editor(*instance);
                self.events.push(HostEvent::EditorClosed(*instance));
            }
        }
        if !applied.created.is_empty() || !applied.removed.is_empty() {
            self.sync_instances();
            self.take_notices();
        }
        if applied.engine || !applied.created.is_empty() || !applied.removed.is_empty() {
            self.prepare();
            self.refresh_problems();
        }
        if applied.graph {
            self.audio.apply_graph(&self.setup);
        }
        if applied.graph || applied.routing {
            // After the graph: a routing names a sink of the newest graph. A rename moves
            // the channel's apps to its new sink name too.
            self.audio.reroute(&self.setup);
            self.refresh_apps();
        }
        if applied.save {
            self.autosave.touch(now);
            self.events.push(HostEvent::SetupChanged);
        }
        Ok(applied)
    }

    fn poll(&mut self) -> Vec<HostEvent> {
        let now = Instant::now();
        let heard = self.audio.poll(&self.setup, &self.engine, now);
        self.heard(heard);
        self.engine.reap();
        self.instances.reap();
        for event in self.instances.tick() {
            match event {
                // The edit already went to the processor; the stored state is stale.
                InstanceEvent::Edit { .. } => self.autosave.touch(now),
                InstanceEvent::Dirty(_) => {
                    self.autosave.touch(now);
                    self.events.push(HostEvent::SetupChanged);
                }
                // The engine sets it up again once no running graph holds it.
                InstanceEvent::IoChanged(instance) => {
                    self.engine.set_up_again(instance);
                }
            }
        }
        let kept = self.kept.len();
        self.kept.retain(|k| k.until > now);
        if self.kept.len() != kept {
            self.sync_instances();
        }
        // What the rack holds moved past what the running graph was prepared against (an
        // instance made, retired or reaped): prepare again so the engine asks.
        if self.rack.generation() != self.engine.lender_generation() {
            self.prepare();
        }
        self.take_notices();
        self.refresh_problems();
        let state = self.audio.state();
        if state != self.backend {
            self.backend = state.clone();
            self.events.push(HostEvent::Backend(state));
        }
        if self.autosave.due(now) {
            self.save_now();
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
            let sends = bank.strip(index);
            *levels = ChannelLevels {
                monitor: stereo(sends[MONITOR].take()),
                master: stereo(sends[MASTER].take()),
            };
        }
        frame.monitor = stereo(bank.bus(MONITOR).take());
        frame.master = stereo(bank.bus(MASTER).take());
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
        self.audio.devices()
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
            let instance = session.instance();
            out.extend(session.tick().into_iter().map(|e| (instance, e)));
        }
        out
    }

    fn resize_editor(&mut self, instance: InstanceId, size: EditorSize) {
        let Some(session) = self.editors.iter_mut().find(|s| s.instance() == instance) else {
            return;
        };
        if let Err(refused) = session.resize(size) {
            log::warn!("{refused}");
        }
    }

    fn close_editor(&mut self, instance: InstanceId) {
        if let Some(at) = self.editors.iter().position(|s| s.instance() == instance) {
            let mut session = self.editors.remove(at);
            session.close();
        }
    }

    fn save_now(&mut self) {
        self.capture_all();
        self.write();
    }

    fn problems(&self) -> &[InsertProblem] {
        &self.problems
    }

    fn set_favorite(&mut self, plugin_id: &str, favorite: bool) {
        if self.catalog.favorites.contains(plugin_id) != favorite {
            self.catalog.set_favorite(plugin_id, favorite);
            self.write_catalog();
        }
    }

    fn rescan_plugins(&mut self) {
        self.scan(true);
        self.instances.retry_failed();
        self.sync_instances();
        self.prepare();
        self.take_notices();
        self.refresh_problems();
        self.events.push(HostEvent::PluginsRescanned);
    }

    fn next_poll(&self) -> Option<Duration> {
        let now = Instant::now();
        let mut at: Option<Instant> = None;
        let mut sooner = |t: Instant| at = Some(at.map_or(t, |a| a.min(t)));
        if let Some(due) = self.autosave.due_at() {
            sooner(due);
        }
        if let Some(until) = self.kept.iter().map(|k| k.until).min() {
            sooner(until);
        }
        // An open editor's edits travel on each tick; a retired instance is reaped on one.
        if !self.editors.is_empty() || self.instances.retiring() > 0 {
            sooner(now + POLL_INTERVAL);
        }
        if let Some(wait) = self.audio.next_poll(now) {
            sooner(now + wait);
        }
        at.map(|t| t.saturating_duration_since(now))
    }
}

impl Drop for RealHost {
    /// Shutdown, in the kit's order: every editor closes before its window and its instance
    /// go; the routings are cleared and the backend leaves, so no tick runs again; the final
    /// save, while every plugin is still live to give its state; an empty graph lets every
    /// lent instance come home, here; then every instance is retired and reaped here,
    /// processor before controller.
    fn drop(&mut self) {
        for session in &mut self.editors {
            session.close();
        }
        self.editors.clear();
        self.audio.shutdown();
        let changed = self.capture_all();
        if changed || self.autosave.pending() {
            self.write();
        }
        self.engine.prepare(&snapshot(&nothing()), &*self.rack);
        self.engine.reap();
        self.kept.clear();
        let live: Vec<InstanceId> = self.instances.live().collect();
        for instance in live {
            self.instances.retire(instance);
        }
        self.instances.reap();
        for event in self.events.drain(..) {
            if let HostEvent::Toast(message) = event {
                log::warn!("{message}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{NULL_CHOSEN, NULL_QUANTUM};
    use crate::autosave::DEBOUNCE;
    use vixmix_model::{ChannelId, ChannelKind, example};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vixmix-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn options(dir: &Path) -> BootOptions {
        BootOptions {
            config_dir: Some(dir.to_path_buf()),
            debug_build: true,
            audio: AudioBackend::Null,
            plugin_dirs: Some(Vec::new()),
        }
    }

    fn boot(dir: &Path) -> RealHost {
        RealHost::boot(options(dir)).unwrap()
    }

    fn banner(events: &[HostEvent]) -> Option<String> {
        events.iter().find_map(|e| match e {
            HostEvent::Backend(state) => Some(state.banner().unwrap_or_default().to_string()),
            _ => None,
        })
    }

    #[test]
    fn it_boots_on_the_null_backend_and_says_so_first() {
        let dir = scratch("boot");
        let mut host = boot(&dir);
        let events = host.poll();
        assert_eq!(banner(&events).as_deref(), Some(NULL_CHOSEN));
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
        assert_eq!(
            host.next_poll(),
            None,
            "with nothing pending it waits for nothing"
        );
        drop(host);
        assert!(
            !dir.exists(),
            "booting and quitting unchanged writes nothing"
        );
    }

    #[test]
    fn a_size_for_an_instance_with_no_open_editor_is_ignored() {
        let dir = scratch("resize");
        let mut host = boot(&dir);
        let _ = host.poll();
        let size = EditorSize {
            width: 800,
            height: 300,
        };
        host.resize_editor(InstanceId(1), size);
        assert_eq!(host.poll(), Vec::new(), "nothing to say, nothing changed");
        assert_eq!(host.next_poll(), None);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn the_system_backend_where_there_is_none_is_the_null_backend_with_its_banner() {
        let dir = scratch("system");
        let mut host = RealHost::boot(BootOptions {
            audio: AudioBackend::APP,
            ..options(&dir)
        })
        .unwrap();
        assert_eq!(
            banner(&host.poll()).as_deref(),
            Some(BackendState::NO_BACKEND)
        );
    }

    #[test]
    fn a_change_is_saved_once_the_setup_has_been_quiet_and_restored_at_the_next_boot() {
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
        assert!(!dir.exists(), "not before the debounce");
        let wait = host.next_poll().expect("a save is pending");
        assert!(wait <= DEBOUNCE, "{wait:?}");
        std::thread::sleep(wait + Duration::from_millis(20));
        let _ = host.poll();
        assert!(persist::path(&dir).exists(), "saved after the debounce");
        let saved = host.setup().clone();
        drop(host);
        let host = boot(&dir);
        assert_eq!(host.setup(), &saved);
        assert!(host.setup().channels[0].master.mute);
        drop(host);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quitting_with_a_change_pending_saves_it() {
        let dir = scratch("quit");
        let mut host = boot(&dir);
        let _ = host.apply(Command::SetPreview(true)).unwrap();
        drop(host);
        let host = boot(&dir);
        assert!(host.setup().preview_master);
        drop(host);
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
        drop(host);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_removed_channel_comes_back_through_the_undo_the_host_hands_out() {
        let dir = scratch("undo");
        let mut host = boot(&dir);
        let ch = host
            .apply(Command::AddChannel {
                kind: ChannelKind::Apps,
            })
            .unwrap()
            .channel
            .unwrap();
        let _ = host
            .apply(Command::Rename {
                ch,
                name: "Music".into(),
            })
            .unwrap();
        let before = host.setup().clone();
        let _ = host.poll();
        let _ = host.apply(Command::RemoveChannel(ch)).unwrap();
        let events = host.poll();
        let undo = events
            .iter()
            .find_map(|e| match e {
                HostEvent::Removed { message, undo } => {
                    assert_eq!(message, "Removed channel Music");
                    Some(undo.clone())
                }
                _ => None,
            })
            .expect("a removal hands out its undo");
        let applied = host.apply(undo).unwrap();
        assert_eq!(applied.channel, Some(ch));
        assert_eq!(host.setup().channels, before.channels);
        // A refused command changes nothing.
        assert!(host.apply(Command::RemoveChannel(ChannelId(99))).is_err());
        assert_eq!(host.setup().channels, before.channels);
        drop(host);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_plugin_is_said_once_and_its_insert_plays_through() {
        let dir = scratch("missing");
        let mut host = boot(&dir);
        let strip = Strip::Channel(host.setup().channels[0].id);
        let _ = host
            .apply(Command::AddInsert {
                strip,
                plugin: example::plugin("Pro-Q 3"),
                at: 0,
            })
            .unwrap();
        let events = host.poll();
        let toasts = events
            .iter()
            .filter(|e| matches!(e, HostEvent::Toast(t) if t.contains("rescan plugins")))
            .count();
        assert_eq!(toasts, 1, "{events:?}");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, HostEvent::Problems(p) if p.len() == 1)),
            "{events:?}"
        );
        assert_eq!(host.problems().len(), 1);
        assert!(host.problems()[0].message.contains(':'));
        let _ = host.apply(Command::SetPreview(true)).unwrap();
        let again = host.poll();
        assert!(
            !again.iter().any(|e| matches!(e, HostEvent::Toast(_))),
            "said once: {again:?}"
        );
        drop(host);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_failures_make_one_toast_that_counts_the_rest() {
        assert_eq!(failures(&[]), None);
        let many: Vec<String> = (0..5).map(|n| format!("B{n}.vst3: broken")).collect();
        let toast = failures(&many).unwrap();
        assert!(
            toast.starts_with("B0.vst3: broken; B1.vst3: broken"),
            "{toast}"
        );
        assert!(toast.contains("and 3 more"), "{toast}");
    }
}
