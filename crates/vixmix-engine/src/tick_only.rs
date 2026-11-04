//! [`TickOnly`]: state the host thread builds and drops, and only the tick touches in
//! between. It is the one place this crate shares something that is not `Sync` (a chain, a
//! lent processor, the tick's working buffer) between the host thread and the data thread.

use std::cell::UnsafeCell;

/// A value with two owners in turn: the host thread, which builds it before publishing it and
/// drops it once no tick can reach it, and the tick, which mutates it in between.
///
/// It hands out only a raw pointer; each caller dereferences it beside a `SAFETY` comment that
/// says why it is the only one. The engine guarantees that by two rules:
///
/// - **One tick at a time.** [`Engine::process`](crate::Engine::process) takes
///   `Engine::ticking` with an atomic swap before it loads a graph and releases it after the
///   graph's guard is gone; a second, concurrent call plays silence instead of touching
///   anything here.
/// - **The host never touches a published value.** A graph is built before the swap that
///   publishes it and dropped only after the acknowledgement or the quiescence check
///   (`Engine::settle`) shows no tick can still reach it; a lent instance is set up before any
///   graph names it and set up again only through `Arc::get_mut`, which proves no graph holds
///   it.
pub(crate) struct TickOnly<T>(UnsafeCell<T>);

// SAFETY: `TickOnly` adds `Sync` to a `Send` value by serialising access rather than by
// locking. Before publication and after retirement only the host thread holds it (it is built
// and dropped there, and `get_mut` needs `&mut`); while published only the tick dereferences
// `get`, and the tick is exclusive (`Engine::ticking`). This is the kit's thread table for a
// VST3 processor (handbook/reference/vst3-host.md § Threads): the audio thread calls
// `process` only, the chain-preparing thread sets up, activates and drops. `T: Send` because
// the value does move between the two threads.
unsafe impl<T: Send> Sync for TickOnly<T> {}

impl<T> TickOnly<T> {
    /// Wraps `value`, which only the host thread holds until it is published.
    pub(crate) fn new(value: T) -> TickOnly<T> {
        TickOnly(UnsafeCell::new(value))
    }

    /// The tick's way in. Dereferencing the pointer is sound only on the tick, holding
    /// `Engine::ticking`, with no other reference from this cell alive.
    pub(crate) fn get(&self) -> *mut T {
        self.0.get()
    }

    /// The host thread's way in, when it holds the value exclusively (`&mut`: before
    /// publication, or through `Arc::get_mut` once no graph holds it).
    pub(crate) fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }
}
