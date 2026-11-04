//! [`Instances`]: the live plugin instances, on the UI thread. Each is made once from its
//! bundle (modules cached by path), loaded with its saved state, split into its two halves —
//! the processor into the [`Rack`] for the engine to borrow, the controller kept here with the
//! UI end of the instance's edit ring — and torn down here in the kit's order: processor
//! first, then controller.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chimaeraproject_audio_processor::AudioProcessor;
use chimaeraproject_param::{NormalizedValue, ParamId};
use chimaeraproject_vst3_host::{
    ClassId, ControllerEvent, HostContext, Module, PluginState, RESTART_IO_CHANGED, Vst3Controller,
    Vst3Error, Vst3Options,
};
use vixmix_model::{Insert, InstanceId, PluginRef};

use crate::catalog::Catalog;
use crate::editor::EditorSession;
use crate::edits::EditSender;
use crate::rack::Rack;

/// How many notices wait for [`Instances::take_notices`]; the oldest go first.
const NOTICE_LIMIT: usize = 32;

/// What an instance's controller reported since the last [`Instances::tick`].
#[derive(Clone, Debug, PartialEq)]
pub enum InstanceEvent {
    /// The plugin's own editor moved a parameter (`performEdit`). [`Instances::tick`] has
    /// already sent the value to the processor through the instance's edit ring; this tells
    /// the host the setup's stored state is now stale.
    Edit {
        instance: InstanceId,
        param: ParamId,
        value: NormalizedValue,
    },
    /// The plugin's state differs from what was saved: the setup should save.
    Dirty(InstanceId),
    /// The plugin changed its buses (`kIoChanged`): the rack's generation has moved, so the
    /// engine prepares again and the chain sets the instance up anew.
    IoChanged(InstanceId),
}

/// Why an instance could not be made, saved or opened. Each message ends with what to do.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error(
        "{name} ({id}) is not in the plugin catalog: install it or rescan plugins (menu, Rescan plugins); its insert passes audio through until then"
    )]
    Missing { name: String, id: String },
    #[error("there is no live plugin instance {0:?}: create it from the setup's insert first")]
    NoInstance(InstanceId),
    #[error("{name}: {source}")]
    Vst3 {
        name: String,
        #[source]
        source: Vst3Error,
    },
}

struct Live {
    instance: InstanceId,
    /// The rack entry its processor went into.
    serial: u64,
    /// The `vst3:<hex>` it was made as: an insert that comes to name another plugin is a new
    /// instance.
    plugin_id: String,
    name: String,
    edits: EditSender,
    controller: Vst3Controller,
}

/// The live instances and the modules they came from; UI thread only.
pub struct Instances {
    host: HostContext,
    rack: Arc<Rack>,
    modules: Vec<(PathBuf, Module)>,
    live: Vec<Live>,
    /// Controllers whose processors are not home yet; dropped by [`Instances::reap`] after
    /// their processor.
    retired: Vec<Live>,
    /// Inserts already reported as not makeable, by instance and plugin id, until
    /// [`Instances::retry_failed`]; only inserts the last sync named are kept.
    failed: BTreeSet<(InstanceId, String)>,
    notices: Vec<String>,
}

impl Instances {
    /// No instances yet; processors go into `rack`.
    pub fn new(host: HostContext, rack: Arc<Rack>) -> Instances {
        Instances {
            host,
            rack,
            modules: Vec::new(),
            live: Vec::new(),
            retired: Vec::new(),
            failed: BTreeSet::new(),
            notices: Vec::new(),
        }
    }

    /// The rack the processors go into.
    pub fn rack(&self) -> &Arc<Rack> {
        &self.rack
    }

    /// Makes `instance` of `plugin`, found through `catalog`, loads `state` (the bytes a
    /// setup's insert stores) and puts its processor in the rack. An instance already live
    /// under that id is retired first. A state that does not load is not a failure: the
    /// plugin starts from its defaults and a notice says so.
    pub fn create(
        &mut self,
        instance: InstanceId,
        plugin: &PluginRef,
        catalog: &Catalog,
        state: Option<&[u8]>,
    ) -> Result<(), PluginError> {
        let entry = catalog
            .get(&plugin.id)
            .ok_or_else(|| PluginError::Missing {
                name: plugin.name.clone(),
                id: plugin.id.clone(),
            })?;
        let vst3 = |source| PluginError::Vst3 {
            name: plugin.name.clone(),
            source,
        };
        let class = ClassId::parse_hex(plugin.id.strip_prefix("vst3:").unwrap_or(&plugin.id))
            .map_err(vst3)?;
        let at = self.module(&entry.path).map_err(vst3)?;
        let (processor, controller) = self.modules[at]
            .1
            .create(class, &self.host, &Vst3Options::default())
            .map_err(vst3)?;
        if let Some(bytes) = state {
            let loaded = PluginState::decode(bytes).and_then(|state| controller.load_state(&state));
            if let Err(error) = loaded {
                self.notice(format!(
                    "{}: its saved state did not load ({error}); it starts from its defaults, and the next save stores those",
                    plugin.name
                ));
            }
        }
        if self.is_live(instance) {
            self.retire(instance);
        }
        let key = processor.descriptor().id.clone();
        let (serial, edits) = self
            .rack
            .add_entry(instance, key.clone(), Box::new(processor));
        self.live.push(Live {
            instance,
            serial,
            plugin_id: key,
            name: plugin.name.clone(),
            edits,
            controller,
        });
        Ok(())
    }

    /// The index of `bundle`'s module in the cache, loading it the first time.
    fn module(&mut self, bundle: &Path) -> Result<usize, Vst3Error> {
        let at = match self.modules.iter().position(|(path, _)| path == bundle) {
            Some(at) => at,
            None => {
                let module = Module::load(bundle, &self.host)?;
                self.modules.push((bundle.to_path_buf(), module));
                self.modules.len() - 1
            }
        };
        Ok(at)
    }

    /// Makes the live instances match `inserts` (a setup's `inserts()`): retires an instance
    /// no insert names any more, or whose insert now names another plugin; creates each
    /// insert's instance that is not live, with its stored state. An insert whose plugin
    /// cannot be made is reported once, in [`take_notices`](Instances::take_notices), and
    /// its insert passes audio through (the rack lends nothing for it); it is tried again
    /// after [`retry_failed`](Instances::retry_failed). Finishes with a [`reap`](Instances::reap).
    pub fn sync<'a>(&mut self, inserts: impl IntoIterator<Item = &'a Insert>, catalog: &Catalog) {
        let wanted: Vec<&Insert> = inserts.into_iter().collect();
        let names = |instance: InstanceId, plugin_id: &str| {
            wanted
                .iter()
                .any(|i| i.instance == instance && i.plugin.id == plugin_id)
        };
        let stale: Vec<InstanceId> = self
            .live
            .iter()
            .filter(|l| !names(l.instance, &l.plugin_id))
            .map(|l| l.instance)
            .collect();
        for instance in stale {
            self.retire(instance);
        }
        self.failed
            .retain(|(instance, plugin_id)| names(*instance, plugin_id));
        for insert in &wanted {
            let key = (insert.instance, insert.plugin.id.clone());
            if self.is_live(insert.instance) || self.failed.contains(&key) {
                continue;
            }
            if let Err(error) = self.create(
                insert.instance,
                &insert.plugin,
                catalog,
                insert.state.as_deref(),
            ) {
                self.failed.insert(key);
                self.notice(error.to_string());
            }
        }
        self.reap();
    }

    /// Lets the next [`sync`](Instances::sync) try every insert that failed again, and report
    /// it again if it still fails: call it after a rescan.
    pub fn retry_failed(&mut self) {
        self.failed.clear();
    }

    /// What went wrong since the last call that no return value carried, each line ending
    /// with what to do: inserts [`sync`](Instances::sync) could not make, states that did not
    /// load, edits dropped. At most the newest few are kept.
    pub fn take_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }

    fn notice(&mut self, message: String) {
        log::warn!("{message}");
        if self.notices.len() == NOTICE_LIMIT {
            self.notices.remove(0);
        }
        self.notices.push(message);
    }

    /// Whether `instance` is live here.
    pub fn is_live(&self, instance: InstanceId) -> bool {
        self.live.iter().any(|l| l.instance == instance)
    }

    /// The live instances, in the order they were made.
    pub fn live(&self) -> impl Iterator<Item = InstanceId> + '_ {
        self.live.iter().map(|l| l.instance)
    }

    /// Retired instances whose processor is still out in a chain.
    pub fn retiring(&self) -> usize {
        self.retired.len()
    }

    /// The controller of `instance`: parameters, their text, meters.
    pub fn controller(&self, instance: InstanceId) -> Option<&Vst3Controller> {
        self.find(instance).map(|l| &l.controller)
    }

    fn find(&self, instance: InstanceId) -> Option<&Live> {
        self.live.iter().find(|l| l.instance == instance)
    }

    /// The mixer's own move of one of `instance`'s parameters (a generic control for a
    /// plugin without an editor): the controller is told, so the plugin's editor shows it,
    /// and the value goes to the processor through the instance's edit ring, the path an
    /// edit made in the plugin's editor takes.
    pub fn set_param(
        &mut self,
        instance: InstanceId,
        param: ParamId,
        value: NormalizedValue,
    ) -> Result<(), PluginError> {
        let live = self
            .live
            .iter_mut()
            .find(|l| l.instance == instance)
            .ok_or(PluginError::NoInstance(instance))?;
        let value = value.clamp(0.0, 1.0);
        live.controller
            .set_param_normalized(param, value)
            .map_err(|source| PluginError::Vst3 {
                name: live.name.clone(),
                source,
            })?;
        // Said once per instance: a ring that stays full stays full.
        if !live.edits.send(param, value) && live.edits.dropped() == 1 {
            let message = dropped_edit(&live.name);
            self.notice(message);
        }
        Ok(())
    }

    /// `instance`'s state as the bytes a setup's insert stores.
    pub fn save_state(&self, instance: InstanceId) -> Result<Vec<u8>, PluginError> {
        let live = self
            .find(instance)
            .ok_or(PluginError::NoInstance(instance))?;
        live.controller
            .save_state()
            .map(|state| state.encode())
            .map_err(|source| PluginError::Vst3 {
                name: live.name.clone(),
                source,
            })
    }

    /// `instance`'s editor, closed, for the app to open in a window it made. `Ok(None)` when
    /// the plugin has no editor. Close it before retiring the instance.
    pub fn editor(&self, instance: InstanceId) -> Result<Option<EditorSession>, PluginError> {
        let live = self
            .find(instance)
            .ok_or(PluginError::NoInstance(instance))?;
        let editor = live
            .controller
            .create_editor()
            .map_err(|source| PluginError::Vst3 {
                name: live.name.clone(),
                source,
            })?;
        Ok(editor.map(|editor| EditorSession::new(instance, editor)))
    }

    /// No chain may borrow `instance` any more; its controller waits here until
    /// [`reap`](Instances::reap) has dropped its processor.
    pub fn retire(&mut self, instance: InstanceId) {
        self.rack.retire(instance);
        let (gone, kept): (Vec<Live>, Vec<Live>) = std::mem::take(&mut self.live)
            .into_iter()
            .partition(|l| l.instance == instance);
        self.live = kept;
        self.retired.extend(gone);
    }

    /// Drops every retired instance whose processor is home, the processor first and then
    /// its controller, here on the UI thread, and returns their ids. Call it every UI tick,
    /// after the engine's reap.
    pub fn reap(&mut self) -> Vec<InstanceId> {
        let mut dropped = Vec::new();
        for reaped in self.rack.reap_entries() {
            drop(reaped.processor);
            if let Some(at) = self.retired.iter().position(|l| l.serial == reaped.serial) {
                drop(self.retired.remove(at));
            }
            dropped.push(reaped.instance);
        }
        dropped
    }

    /// Drains every controller's events: an edit made in a plugin's own editor goes to its
    /// processor through the edit ring and is reported; dirty state is reported; a bus change
    /// moves the rack's generation and is reported. Values waiting for room in a ring are
    /// flushed, and the meters the processors reported are drained into their controllers.
    /// Call it every UI tick.
    pub fn tick(&mut self) -> Vec<InstanceEvent> {
        let mut out = Vec::new();
        let mut touch = false;
        let mut dropped = Vec::new();
        for live in &mut self.live {
            live.edits.flush();
            for event in live.controller.drain_events() {
                let before = live.edits.dropped();
                out.extend(route(live.instance, event, &mut live.edits, &mut touch));
                if before == 0 && live.edits.dropped() > 0 {
                    dropped.push(dropped_edit(&live.name));
                }
            }
            let _ = live.controller.poll_output_changes();
        }
        if touch {
            self.rack.touch();
        }
        for message in dropped {
            self.notice(message);
        }
        out
    }
}

impl Drop for Instances {
    /// Every instance retired and reaped, processors before controllers. A processor still
    /// lent (the engine's chains were not dropped first) cannot be reaped, and its controller
    /// goes now, before it: the log says which order to keep.
    fn drop(&mut self) {
        let live: Vec<InstanceId> = self.live().collect();
        for instance in live {
            self.retire(instance);
        }
        self.reap();
        if !self.retired.is_empty() {
            log::warn!(
                "{} plugin instance(s) were still in a chain when the instances were dropped: drop the engine's graphs, then reap, before dropping the instances",
                self.retired.len()
            );
        }
    }
}

/// What one controller event means: an edit is sent to the processor and reported, a dirty
/// state is reported, a bus change asks for a new rack generation (`touch`) and is reported;
/// the rest (gestures, a latency restart the kit already refreshed, editor requests) are not
/// the mixer's.
fn route(
    instance: InstanceId,
    event: ControllerEvent,
    edits: &mut EditSender,
    touch: &mut bool,
) -> Option<InstanceEvent> {
    match event {
        ControllerEvent::PerformEdit { id, value } => {
            edits.send(id, value);
            Some(InstanceEvent::Edit {
                instance,
                param: id,
                value,
            })
        }
        ControllerEvent::Dirty(true) => Some(InstanceEvent::Dirty(instance)),
        ControllerEvent::Restart(flags) if flags & RESTART_IO_CHANGED != 0 => {
            *touch = true;
            Some(InstanceEvent::IoChanged(instance))
        }
        _ => None,
    }
}

fn dropped_edit(name: &str) -> String {
    format!(
        "{name}: more of its parameters changed than can wait while its insert is in no running chain, so some changes were dropped; enable the insert or start the audio, then set them again"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixmix_engine::Lender;

    fn missing(instance: u32, name: &str) -> Insert {
        Insert {
            instance: InstanceId(instance),
            plugin: PluginRef {
                id: format!("vst3:{}", "0".repeat(32)),
                bundle: format!("{name}.vst3"),
                name: name.to_string(),
            },
            enabled: true,
            state: None,
        }
    }

    #[test]
    fn a_plugin_the_catalog_lacks_is_refused_with_the_fix_and_nothing_is_racked() {
        let rack = Rack::new();
        let mut instances = Instances::new(HostContext::new("vix-mix"), Arc::clone(&rack));
        let plugin = missing(1, "Gone").plugin;
        let error = instances
            .create(InstanceId(1), &plugin, &Catalog::default(), None)
            .unwrap_err();
        assert!(error.to_string().contains("rescan plugins"), "{error}");
        assert!(!rack.holds(InstanceId(1)));
        assert!(!instances.is_live(InstanceId(1)));
        assert!(matches!(
            instances.save_state(InstanceId(1)),
            Err(PluginError::NoInstance(_))
        ));
    }

    #[test]
    fn a_missing_plugin_is_reported_once_until_a_retry_and_lends_nothing() {
        let rack = Rack::new();
        let mut instances = Instances::new(HostContext::new("vix-mix"), Arc::clone(&rack));
        let inserts = [missing(1, "Gone"), missing(2, "Also Gone")];
        let catalog = Catalog::default();
        instances.sync(&inserts, &catalog);
        let notices = instances.take_notices();
        assert_eq!(notices.len(), 2, "{notices:?}");
        assert!(notices[0].contains("Gone") && notices[0].contains("passes audio through"));
        // The engine asks every prepare; the insert is a pass-through, not an error.
        assert!(
            rack.lend(InstanceId(1), &inserts[0].plugin.id)
                .unwrap()
                .is_none()
        );
        instances.sync(&inserts, &catalog);
        assert!(instances.take_notices().is_empty(), "reported once");
        // An insert the setup dropped is forgotten; a rescan lets the rest be tried again.
        instances.sync(&inserts[1..], &catalog);
        instances.retry_failed();
        instances.sync(&inserts, &catalog);
        assert_eq!(instances.take_notices().len(), 2);
        assert_eq!(rack.generation(), 0, "nothing was racked");
    }

    #[test]
    fn controller_events_become_instance_events_and_edits_go_to_the_ring() {
        let (mut edits, mut receiver) = crate::edits::ring();
        let instance = InstanceId(9);
        let mut touch = false;
        let events = [
            ControllerEvent::BeginEdit(4),
            ControllerEvent::PerformEdit { id: 4, value: 0.75 },
            ControllerEvent::EndEdit(4),
            ControllerEvent::Dirty(true),
            ControllerEvent::Dirty(false),
            ControllerEvent::Restart(8),
            ControllerEvent::RequestOpenEditor("editor".to_string()),
        ];
        let out: Vec<InstanceEvent> = events
            .into_iter()
            .filter_map(|e| route(instance, e, &mut edits, &mut touch))
            .collect();
        assert_eq!(
            out,
            [
                InstanceEvent::Edit {
                    instance,
                    param: 4,
                    value: 0.75
                },
                InstanceEvent::Dirty(instance)
            ]
        );
        assert!(!touch, "a latency restart is not a bus change");
        let sent = receiver
            .pop()
            .expect("the edit is on its way to the processor");
        assert_eq!((sent.id, sent.normalized, sent.sample_offset), (4, 0.75, 0));
        assert!(receiver.pop().is_err());

        let io = route(
            instance,
            ControllerEvent::Restart(RESTART_IO_CHANGED),
            &mut edits,
            &mut touch,
        );
        assert_eq!(io, Some(InstanceEvent::IoChanged(instance)));
        assert!(touch);
    }
}
