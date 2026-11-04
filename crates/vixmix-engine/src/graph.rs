//! [`Prepared`]: one graph, built on the host thread from a snapshot and the registry's
//! instances, then only read (and its chains run) by the tick until the host drops it.

use std::sync::Arc;

use chimaeraproject_audio_processor::{
    AudioProcessor, BusLayout, EngineConfig, ProcessSetup, ProcessorChain, ProcessorInstanceId,
    ProcessorParams,
};
use vixmix_model::{EngineSnapshot, InsertSnap, InstanceId, MAX_CHANNELS, SendSnap, Strip};

use crate::instance::Shared;
use crate::registry::{self, HeldState, Registry};
use crate::tick_only::TickOnly;

/// An insert the running graph plays as a pass-through, and why. The engine keeps one per
/// failing insert in the graph it prepared, so a failure is reported once (as state the host
/// reads with [`Engine::problems`](crate::Engine::problems)) and never retried per block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertProblem {
    /// The strip whose chain names the insert.
    pub strip: Strip,
    pub instance: InstanceId,
    /// `vst3:<hex>`, as the snapshot names it.
    pub plugin_id: String,
    /// What went wrong and what to do about it.
    pub message: String,
}

/// A send or level as the tick applies it: `None` contributes nothing (mute beats everything).
pub(crate) fn audible_gain(send: &SendSnap) -> Option<f32> {
    send.audible().then_some(send.gain)
}

/// A channel's routing and sends; its chain is in [`Chains`].
pub(crate) struct PreparedChannel {
    pub(crate) monitor: Option<f32>,
    pub(crate) master: Option<f32>,
    /// MON: the Monitor send takes the chain's output rather than the dry input.
    pub(crate) chain_to_monitor: bool,
    /// MST: the Master send takes the chain's output.
    pub(crate) chain_to_master: bool,
}

/// One strip's chain as the tick runs it: the enabled inserts whose instance is ready, each
/// with its `ProcessorParams` (an id and a plugin id, no values: see `reference/engine.md`
/// § Parameters).
pub(crate) struct StripChain {
    pub(crate) chain: ProcessorChain,
    pub(crate) params: Vec<ProcessorParams>,
}

impl StripChain {
    /// Whether the tick has anything to run: an empty chain passes audio through untouched.
    pub(crate) fn is_live(&self) -> bool {
        !self.chain.is_empty()
    }
}

/// Everything the tick mutates in a graph.
pub(crate) struct Chains {
    pub(crate) channels: Vec<StripChain>,
    pub(crate) monitor: StripChain,
    pub(crate) master: StripChain,
}

/// One graph, built on the host thread from a snapshot and only ever run by the tick.
pub(crate) struct Prepared {
    pub(crate) generation: u64,
    /// The lender's generation this graph was prepared against.
    pub(crate) lender_generation: u64,
    /// The rate its instances were set up at: the DSP load's budget.
    pub(crate) sample_rate: u32,
    /// At most [`MAX_CHANNELS`], in the snapshot's order, as long as `chains.channels`.
    pub(crate) channels: Vec<PreparedChannel>,
    pub(crate) monitor: Option<f32>,
    pub(crate) master: Option<f32>,
    pub(crate) preview_master: bool,
    pub(crate) problems: Vec<InsertProblem>,
    /// The chains, which only the tick runs (`TickOnly`).
    pub(crate) chains: TickOnly<Chains>,
}

impl Prepared {
    /// The graph `snapshot` describes, with every insert's instance as `registry` holds it.
    /// `None` (nothing prepared yet) is silence.
    pub(crate) fn build(
        generation: u64,
        config: &EngineConfig,
        snapshot: Option<&EngineSnapshot>,
        registry: &Registry,
    ) -> Prepared {
        let setup = ProcessSetup::realtime(config);
        let mut builder = Builder {
            setup,
            registry,
            seen: Vec::new(),
            problems: Vec::new(),
        };
        let (channels, chains, monitor, master, preview_master) = match snapshot {
            None => (
                Vec::new(),
                Chains {
                    channels: Vec::new(),
                    monitor: builder.empty(),
                    master: builder.empty(),
                },
                None,
                None,
                false,
            ),
            Some(snap) => {
                let mut channels = Vec::new();
                let mut strips = Vec::new();
                for channel in snap.channels.iter().take(MAX_CHANNELS) {
                    channels.push(PreparedChannel {
                        monitor: audible_gain(&channel.monitor),
                        master: audible_gain(&channel.master),
                        chain_to_monitor: channel.chain_to_monitor,
                        chain_to_master: channel.chain_to_master,
                    });
                    strips.push(builder.strip(Strip::Channel(channel.id), &channel.chain));
                }
                let chains = Chains {
                    channels: strips,
                    monitor: builder.strip(Strip::Mix(snap.monitor.id), &snap.monitor.chain),
                    master: builder.strip(Strip::Mix(snap.master.id), &snap.master.chain),
                };
                (
                    channels,
                    chains,
                    audible_gain(&snap.monitor.level),
                    audible_gain(&snap.master.level),
                    snap.preview_master,
                )
            }
        };
        Prepared {
            generation,
            lender_generation: registry.lender_generation(),
            sample_rate: config.sample_rate,
            channels,
            monitor,
            master,
            preview_master,
            problems: builder.problems,
            chains: TickOnly::new(chains),
        }
    }
}

/// Every insert of `snapshot`, in the order the graph runs them: the channels' (at most
/// [`MAX_CHANNELS`]), then Monitor's, then Master's. What the registry resolves.
pub(crate) fn inserts(snapshot: &EngineSnapshot) -> impl Iterator<Item = &InsertSnap> {
    snapshot
        .channels
        .iter()
        .take(MAX_CHANNELS)
        .flat_map(|c| &c.chain)
        .chain(&snapshot.monitor.chain)
        .chain(&snapshot.master.chain)
}

struct Builder<'a> {
    setup: ProcessSetup,
    registry: &'a Registry,
    /// Instances already placed in this graph: a second slot naming one is refused.
    seen: Vec<InstanceId>,
    problems: Vec<InsertProblem>,
}

impl Builder<'_> {
    fn empty(&self) -> StripChain {
        StripChain {
            chain: ProcessorChain::new(self.setup, BusLayout::stereo_insert()),
            params: Vec::new(),
        }
    }

    /// One strip's chain: every enabled insert whose instance is ready, filled through the
    /// kit's `fill_with` with a handle onto the shared instance. A bypassed insert is left
    /// out (its instance stays lent and set up, so turning it back on is immediate); one
    /// that failed, or is being set up again, is left out as a pass-through, and a failure is
    /// recorded as an [`InsertProblem`].
    fn strip(&mut self, strip: Strip, inserts: &[InsertSnap]) -> StripChain {
        let mut params = Vec::new();
        let mut lent: Vec<Arc<_>> = Vec::new();
        for insert in inserts {
            let resolved = if self.seen.contains(&insert.instance) {
                Err(registry::duplicate(insert.instance, &insert.plugin_id))
            } else {
                self.seen.push(insert.instance);
                match self.registry.state(insert.instance, &insert.plugin_id) {
                    Some(HeldState::Ready(cell)) => Ok(Some(cell)),
                    Some(HeldState::Resetting(_)) => Ok(None),
                    Some(HeldState::Failed(message)) => Err(message.clone()),
                    // The registry resolves every insert of the snapshot it builds from.
                    Some(HeldState::Leaving { .. }) | None => {
                        Err(registry::missing(insert.instance, &insert.plugin_id))
                    }
                }
            };
            match resolved {
                Ok(Some(cell)) if insert.enabled => {
                    params.push(ProcessorParams::new(
                        processor_id(insert.instance),
                        insert.plugin_id.clone(),
                    ));
                    lent.push(Arc::clone(cell));
                }
                Ok(_) => {}
                Err(message) => self.problems.push(InsertProblem {
                    strip,
                    instance: insert.instance,
                    plugin_id: insert.plugin_id.clone(),
                    message,
                }),
            }
        }
        let mut chain = ProcessorChain::new(self.setup, BusLayout::stereo_insert());
        let mut lent = lent.into_iter();
        chain.fill_with(&params, |_, _| {
            // One handle per entry of `params`, in order: `fill_with` asks once per insert.
            let cell = lent
                .next()
                .expect("a handle for every insert the chain asks for");
            Ok(Box::new(Shared(cell)) as Box<dyn AudioProcessor>)
        });
        // A handle accepts the layout and setup its instance runs at, which are this chain's:
        // the registry sets every ready instance up at the engine's current setup.
        debug_assert_eq!(
            chain.errors().count(),
            0,
            "a shared instance refused its chain"
        );
        StripChain { chain, params }
    }
}

/// The kit chain's id for an insert: the model's instance id, widened.
pub(crate) fn processor_id(instance: InstanceId) -> ProcessorInstanceId {
    ProcessorInstanceId(u64::from(instance.0))
}
