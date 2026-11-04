//! [`Instance`]: one lent plugin processor, set up once at the engine's rate and shared by
//! every graph that names it, and [`Shared`], the forwarding processor a chain holds.
//!
//! **Why share.** `prepare` builds the next graph on the host thread while the tick is still
//! running the current one, whose chains hold every lent instance. Lending again would find
//! each instance `Busy`, and taking it out of the running chain cannot be done from the host
//! thread. So the engine lends an instance once, keeps it in an `Instance` cell, and every
//! graph's chain holds a [`Shared`] handle onto that cell. Only one graph runs per tick and
//! only the tick calls `process`, so the cell is never processed twice at once; a graph that
//! adds, removes, moves or bypasses an insert reuses the same instance with its runtime state
//! and no new setup.
//!
//! **Who sets it up.** The engine, not the chain: [`Instance::set_up`] runs `set_bus_layout`,
//! `setup_processing` and `set_active(true)` on the host thread before any graph names it,
//! and [`Instance::set_up_again`] repeats them only through `Arc::get_mut`, once no graph holds
//! the cell. The chain still calls those three on its [`Shared`] handle when it inserts it;
//! the handle answers from the cell's record and never reaches the plugin, which may be
//! processing in the running graph at that moment.

use std::sync::Arc;

use chimaeraproject_audio_processor::{
    AudioError, AudioProcessData, AudioProcessor, BusLayout, ProcessSetup, ProcessorDescriptor,
};

use crate::tick_only::TickOnly;

/// A lent processor, set up and active, shared by the graphs that name it.
pub(crate) struct Instance {
    /// The lender's processor (in the plugins crate, its forwarding handle). Touched by the
    /// tick through [`Shared::process`], and by the host thread only before publication or
    /// through `Arc::get_mut`.
    processor: TickOnly<Box<dyn AudioProcessor>>,
    /// Copied at setup, so a chain built on the host thread never reads the plugin while the
    /// tick may be processing it.
    descriptor: ProcessorDescriptor,
    latency: u32,
    layout: BusLayout,
    setup: ProcessSetup,
}

impl Instance {
    /// Host thread: sets `processor` up for `layout` and `setup` and activates it. On a
    /// refusal the processor is dropped here, which sends a lent one home.
    pub(crate) fn set_up(
        mut processor: Box<dyn AudioProcessor>,
        layout: &BusLayout,
        setup: &ProcessSetup,
    ) -> Result<Instance, AudioError> {
        processor.set_bus_layout(layout)?;
        processor.setup_processing(setup)?;
        processor.set_active(true);
        Ok(Instance {
            descriptor: processor.descriptor().clone(),
            latency: processor.latency_samples(),
            processor: TickOnly::new(processor),
            layout: layout.clone(),
            setup: *setup,
        })
    }

    /// Host thread, holding the cell exclusively (`Arc::get_mut`, so no graph can reach it):
    /// deactivates, sets up for `layout` and `setup` again and reactivates. What a new rate or
    /// a plugin that changed its buses needs.
    pub(crate) fn set_up_again(
        &mut self,
        layout: &BusLayout,
        setup: &ProcessSetup,
    ) -> Result<(), AudioError> {
        let processor = self.processor.get_mut();
        processor.set_active(false);
        processor.set_bus_layout(layout)?;
        processor.setup_processing(setup)?;
        processor.set_active(true);
        self.descriptor = processor.descriptor().clone();
        self.latency = processor.latency_samples();
        self.layout = layout.clone();
        self.setup = *setup;
        Ok(())
    }

    /// The setup it runs at.
    pub(crate) fn setup(&self) -> &ProcessSetup {
        &self.setup
    }
}

impl Drop for Instance {
    /// Runs on the host thread: the last `Arc` is the host's (a graph is dropped there, and so
    /// is the registry's handle). Deactivates before the processor drops, which sends a lent
    /// one home through the lender's own handle.
    fn drop(&mut self) {
        self.processor.get_mut().set_active(false);
    }
}

/// What a chain's slot holds: a handle onto a shared [`Instance`].
pub(crate) struct Shared(pub(crate) Arc<Instance>);

impl AudioProcessor for Shared {
    fn descriptor(&self) -> &ProcessorDescriptor {
        &self.0.descriptor
    }

    /// Accepts the layout the instance was set up with and refuses any other: the plugin may
    /// be processing in the running graph, so it is never asked here.
    fn set_bus_layout(&mut self, layout: &BusLayout) -> Result<(), AudioError> {
        if *layout == self.0.layout {
            Ok(())
        } else {
            Err(AudioError::UnsupportedBusLayout {
                processor: self.0.descriptor.id.clone(),
                expected: format!(
                    "the layout it was set up with ({})",
                    self.0.layout.describe()
                ),
                found: layout.describe(),
            })
        }
    }

    /// Accepts the setup the instance runs at and refuses any other, for the same reason;
    /// the engine sets an instance up again itself, once no graph holds it.
    fn setup_processing(&mut self, setup: &ProcessSetup) -> Result<(), AudioError> {
        if *setup == self.0.setup {
            Ok(())
        } else {
            Err(AudioError::SetupFailed {
                processor: self.0.descriptor.id.clone(),
                reason: format!(
                    "it runs at {} Hz and {} frames, and this chain asked for {} Hz and {} \
                     frames; the engine sets an instance up again only once no graph holds it",
                    self.0.setup.sample_rate,
                    self.0.setup.max_block,
                    setup.sample_rate,
                    setup.max_block
                ),
            })
        }
    }

    /// Activation is the instance's, not the chain's: a chain inserting or clearing its handle
    /// says nothing about the graph still running it.
    fn set_active(&mut self, _active: bool) {}

    fn process(&mut self, data: &mut AudioProcessData<'_>) {
        // SAFETY: only `ProcessorChain::process` calls this, on a chain inside a published
        // graph, and only `Engine::process` runs those chains, holding `Engine::ticking`, so
        // this is the tick and no other tick is running. The host thread never dereferences
        // the cell while a graph holds it (`TickOnly`'s rules), and the `&mut` made here ends
        // with this call, so it aliases nothing.
        let processor = unsafe { &mut *self.0.processor.get() };
        processor.process(data);
    }

    fn latency_samples(&self) -> u32 {
        self.0.latency
    }
}
