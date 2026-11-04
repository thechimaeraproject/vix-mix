//! [`Instances`]: the live plugin instances, on the UI thread. Each is made once from its
//! bundle (modules cached by path), loaded with its saved state, split into its two halves —
//! the processor into the [`Rack`] for the engine to borrow, the controller kept here — and
//! torn down here in the kit's order: processor first, then controller.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chimaeraproject_audio_processor::AudioProcessor;
use chimaeraproject_param::{NormalizedValue, ParamId};
use chimaeraproject_vst3_host::{
    ClassId, ControllerEvent, HostContext, Module, PluginState, RESTART_IO_CHANGED, Vst3Controller,
    Vst3Error, Vst3Options,
};
use vixmix_model::{InstanceId, PluginRef};

use crate::catalog::Catalog;
use crate::editor::EditorSession;
use crate::rack::Rack;

/// What an instance's controller reported since the last [`Instances::tick`].
#[derive(Clone, Debug, PartialEq)]
pub enum InstanceEvent {
    /// The plugin's own editor moved a parameter (`performEdit`). The kit never short-cuts
    /// an edit to the processor: it reaches it in a later block's parameter changes, through
    /// the engine (handoffs 02 and 04 agree the path).
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
    name: String,
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
        }
    }

    /// The rack the processors go into.
    pub fn rack(&self) -> &Arc<Rack> {
        &self.rack
    }

    /// Makes `instance` of `plugin`, found through `catalog`, loads `state` (the bytes a
    /// setup's insert stores) and puts its processor in the rack.
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
            let state = PluginState::decode(bytes).map_err(vst3)?;
            controller.load_state(&state).map_err(vst3)?;
        }
        let key = processor.descriptor().id.clone();
        self.rack.add(instance, key, Box::new(processor));
        self.live.push(Live {
            instance,
            name: plugin.name.clone(),
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

    /// Whether `instance` is live here.
    pub fn is_live(&self, instance: InstanceId) -> bool {
        self.live.iter().any(|l| l.instance == instance)
    }

    /// The controller of `instance`: parameters, their text, meters.
    pub fn controller(&self, instance: InstanceId) -> Option<&Vst3Controller> {
        self.live
            .iter()
            .find(|l| l.instance == instance)
            .map(|l| &l.controller)
    }

    /// `instance`'s state as the bytes a setup's insert stores.
    pub fn save_state(&self, instance: InstanceId) -> Result<Vec<u8>, PluginError> {
        let live = self
            .live
            .iter()
            .find(|l| l.instance == instance)
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
    /// the plugin has no editor.
    pub fn editor(&self, instance: InstanceId) -> Result<Option<EditorSession>, PluginError> {
        let live = self
            .live
            .iter()
            .find(|l| l.instance == instance)
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

    /// Drops every retired instance whose processor is home: the processor first, then the
    /// controller, here on the UI thread. Call it every UI tick, after the engine's reap.
    pub fn reap(&mut self) {
        for (instance, processor) in self.rack.reap() {
            drop(processor);
            self.retired.retain(|l| l.instance != instance);
        }
    }

    /// Drains every controller's events: edits made in a plugin's own editor, dirty state,
    /// and bus changes (which move the rack's generation here).
    pub fn tick(&mut self) -> Vec<InstanceEvent> {
        let mut out = Vec::new();
        for live in &mut self.live {
            for event in live.controller.drain_events() {
                match event {
                    ControllerEvent::PerformEdit { id, value } => out.push(InstanceEvent::Edit {
                        instance: live.instance,
                        param: id,
                        value,
                    }),
                    ControllerEvent::Dirty(true) => out.push(InstanceEvent::Dirty(live.instance)),
                    ControllerEvent::Restart(flags) if flags & RESTART_IO_CHANGED != 0 => {
                        self.rack.touch();
                        out.push(InstanceEvent::IoChanged(live.instance));
                    }
                    _ => {}
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_the_catalog_lacks_is_refused_with_the_fix_and_nothing_is_racked() {
        let rack = Rack::new();
        let mut instances = Instances::new(HostContext::new("vix-mix"), Arc::clone(&rack));
        let plugin = PluginRef {
            id: format!("vst3:{}", "0".repeat(32)),
            bundle: "Gone.vst3".to_string(),
            name: "Gone".to_string(),
        };
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
}
