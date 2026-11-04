//! [`Registry`]: what the engine holds of the lender's instances between graphs, on the host
//! thread. One entry per `(instance, plugin id)` the snapshot names, plus the instances a
//! running graph may still hold after the snapshot stopped naming them.
//!
//! **Lending once.** An instance is lent the first time a snapshot names it and kept in an
//! [`Instance`] cell every later graph shares (see `instance.rs` for why). The lender is asked
//! again only when its generation moves: an instance the engine already holds answers
//! `Busy` (it is lent, to us) and is kept; `Ok(Some(_))` means the lender holds a new instance
//! under that id (the old one was retired), which replaces it; `Ok(None)` or `OtherPlugin`
//! turn the insert into a reported pass-through.
//!
//! **Failures are kept**, with their message, and tried again only when the lender's
//! generation moves, so a missing plugin is not looked up on every fader move.

use std::sync::{Arc, Weak};

use chimaeraproject_audio_processor::{AudioProcessor, BusLayout, ProcessSetup};
use vixmix_model::InstanceId;

use crate::instance::Instance;
use crate::lender::{LendError, Lender};

/// Where one named instance stands.
pub(crate) enum HeldState {
    /// Lent, set up at the engine's rate and active: every new graph that names it shares it.
    Ready(Arc<Instance>),
    /// To be set up again (a new rate, new buses): left out of every new graph, its insert a
    /// pass-through, until no running graph holds it; then set up in place and ready again.
    Resetting(Arc<Instance>),
    /// The snapshot stopped naming it while a running graph may still hold it. It goes home
    /// when the last graph holding it is dropped; named again before that, it is taken back
    /// rather than lent again (which would find it `Busy`). `reset` remembers a pending
    /// set-up-again.
    Leaving { cell: Weak<Instance>, reset: bool },
    /// Not lent or not set up, and why: the insert is a reported pass-through.
    Failed(String),
}

struct Held {
    instance: InstanceId,
    plugin_id: String,
    state: HeldState,
}

/// The engine's holdings, touched only by the host thread under `Engine`'s host lock.
#[derive(Default)]
pub(crate) struct Registry {
    held: Vec<Held>,
    /// The lender's generation at the last resolve; `None` before the first.
    lender_generation: Option<u64>,
}

/// What [`Registry::resolve`] sets a newly lent instance up with.
pub(crate) struct SetUpWith<'a> {
    pub(crate) layout: &'a BusLayout,
    pub(crate) setup: &'a ProcessSetup,
}

impl Registry {
    /// The lender's generation the holdings were last checked against.
    pub(crate) fn lender_generation(&self) -> u64 {
        self.lender_generation.unwrap_or(0)
    }

    /// Where `(instance, plugin_id)` stands, if the last resolve named it.
    pub(crate) fn state(&self, instance: InstanceId, plugin_id: &str) -> Option<&HeldState> {
        self.held
            .iter()
            .find(|h| h.instance == instance && h.plugin_id == plugin_id)
            .map(|h| &h.state)
    }

    /// Host thread: brings the holdings in line with `named` (every insert of the snapshot,
    /// in order), lending through `lender` what is new and checking what is held when the
    /// lender's generation has moved. A newly lent instance is set up here, before any graph
    /// can name it.
    pub(crate) fn resolve(
        &mut self,
        named: &[(InstanceId, &str)],
        lender: &dyn Lender,
        with: &SetUpWith<'_>,
    ) {
        let generation = lender.generation();
        let moved = self.lender_generation != Some(generation);
        self.lender_generation = Some(generation);
        let is_named = |instance: InstanceId, plugin_id: &str| {
            named.iter().any(|&(i, p)| i == instance && p == plugin_id)
        };

        // What the snapshot no longer names leaves (a running graph may still hold it).
        for held in &mut self.held {
            if !is_named(held.instance, &held.plugin_id) {
                held.state = match std::mem::replace(&mut held.state, placeholder()) {
                    HeldState::Ready(cell) => HeldState::Leaving {
                        cell: Arc::downgrade(&cell),
                        reset: false,
                    },
                    HeldState::Resetting(cell) => HeldState::Leaving {
                        cell: Arc::downgrade(&cell),
                        reset: true,
                    },
                    other => other,
                };
            }
        }
        self.held.retain(|h| match &h.state {
            HeldState::Failed(_) => is_named(h.instance, &h.plugin_id),
            HeldState::Leaving { cell, .. } => cell.strong_count() > 0,
            _ => true,
        });

        for (index, &(instance, plugin_id)) in named.iter().enumerate() {
            if named[..index].contains(&(instance, plugin_id)) {
                continue;
            }
            let ask = Ask {
                lender,
                instance,
                plugin_id,
                moved,
                with,
            };
            let Some(held) = self
                .held
                .iter_mut()
                .find(|h| h.instance == instance && h.plugin_id == plugin_id)
            else {
                self.held.push(Held {
                    instance,
                    plugin_id: plugin_id.to_string(),
                    state: ask.lend(),
                });
                continue;
            };
            held.state = match std::mem::replace(&mut held.state, placeholder()) {
                // Taken back before it went home. One set up at another rate, or with a
                // set-up-again pending, is set up again first.
                HeldState::Leaving { cell, reset } => match cell.upgrade() {
                    Some(cell) => {
                        let reset = reset || cell.setup() != with.setup;
                        ask.check(cell, reset)
                    }
                    None => ask.lend(),
                },
                HeldState::Ready(cell) => ask.check(cell, false),
                HeldState::Resetting(cell) => ask.check(cell, true),
                HeldState::Failed(_) if moved => ask.lend(),
                failed => failed,
            };
        }
    }

    /// Marks every held instance to be set up again (a new rate). Returns whether any was.
    pub(crate) fn reset_all(&mut self) -> bool {
        self.reset_where(|_| true)
    }

    /// Marks `instance` to be set up again (new buses). Returns whether the engine holds it.
    pub(crate) fn reset(&mut self, instance: InstanceId) -> bool {
        self.reset_where(|i| i == instance)
    }

    fn reset_where(&mut self, which: impl Fn(InstanceId) -> bool) -> bool {
        let mut any = false;
        for held in self.held.iter_mut().filter(|h| which(h.instance)) {
            held.state = match std::mem::replace(&mut held.state, placeholder()) {
                HeldState::Ready(cell) | HeldState::Resetting(cell) => {
                    any = true;
                    HeldState::Resetting(cell)
                }
                HeldState::Leaving { cell, .. } => HeldState::Leaving { cell, reset: true },
                failed => failed,
            };
        }
        any
    }

    /// Host thread: sets up again every instance waiting for it that no graph holds any more
    /// (`Arc::get_mut` proves it), and forgets what has gone home. Returns whether any
    /// instance changed state, in which case the graph must be built again to include it.
    pub(crate) fn finish_resets(&mut self, with: &SetUpWith<'_>) -> bool {
        let mut changed = false;
        for held in &mut self.held {
            let HeldState::Resetting(cell) = &mut held.state else {
                continue;
            };
            let Some(instance) = Arc::get_mut(cell) else {
                continue;
            };
            let outcome = instance.set_up_again(with.layout, with.setup);
            held.state = match (outcome, std::mem::replace(&mut held.state, placeholder())) {
                (Ok(()), HeldState::Resetting(cell)) => HeldState::Ready(cell),
                // The cell drops here, on the host thread, and a lent processor goes home.
                (Err(error), _) => HeldState::Failed(error.to_string()),
                (Ok(()), other) => other,
            };
            changed = true;
        }
        self.held.retain(|h| match &h.state {
            HeldState::Leaving { cell, .. } => cell.strong_count() > 0,
            _ => true,
        });
        changed
    }
}

/// One named insert being resolved: who to ask, and what to set a new instance up with.
struct Ask<'a> {
    lender: &'a dyn Lender,
    instance: InstanceId,
    plugin_id: &'a str,
    /// Whether the lender's generation moved since the last resolve.
    moved: bool,
    with: &'a SetUpWith<'a>,
}

impl Ask<'_> {
    /// Lends the instance afresh and sets it up.
    fn lend(&self) -> HeldState {
        match self.lender.lend(self.instance, self.plugin_id) {
            Ok(Some(processor)) => self.set_up(processor),
            Ok(None) => HeldState::Failed(missing(self.instance, self.plugin_id)),
            Err(error) => HeldState::Failed(error.to_string()),
        }
    }

    /// A held instance, checked with the lender when its generation moved: still ours
    /// (`Busy`) it is kept; replaced, missing or now another plugin, the held cell is let go
    /// (it goes home once no running graph holds it).
    fn check(&self, cell: Arc<Instance>, reset: bool) -> HeldState {
        let keep = |cell| {
            if reset {
                HeldState::Resetting(cell)
            } else {
                HeldState::Ready(cell)
            }
        };
        if !self.moved {
            return keep(cell);
        }
        match self.lender.lend(self.instance, self.plugin_id) {
            Err(LendError::Busy { .. }) => keep(cell),
            Ok(Some(processor)) => self.set_up(processor),
            Ok(None) => HeldState::Failed(missing(self.instance, self.plugin_id)),
            Err(error) => HeldState::Failed(error.to_string()),
        }
    }

    fn set_up(&self, processor: Box<dyn AudioProcessor>) -> HeldState {
        match Instance::set_up(processor, self.with.layout, self.with.setup) {
            Ok(instance) => HeldState::Ready(Arc::new(instance)),
            Err(error) => HeldState::Failed(error.to_string()),
        }
    }
}

/// A state to leave behind while the real one is moved out; always overwritten at once.
fn placeholder() -> HeldState {
    HeldState::Failed(String::new())
}

/// Why an insert whose instance the lender does not hold passes audio through.
pub(crate) fn missing(instance: InstanceId, plugin_id: &str) -> String {
    format!(
        "plugin instance {instance:?} ({plugin_id}) is not loaded, so its slot passes audio \
         through untouched: install or rescan the plugin, or remove the insert"
    )
}

/// Why the second slot naming one instance passes audio through.
pub(crate) fn duplicate(instance: InstanceId, plugin_id: &str) -> String {
    format!(
        "plugin instance {instance:?} ({plugin_id}) is already in another slot, so this slot \
         passes audio through: one instance plays in one slot; copy the insert to get a second \
         instance"
    )
}
