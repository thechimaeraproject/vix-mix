# 06 — Host and integration

**Record — not yet built.**
**Needs:** 01, 02, 03, 04. **Runs alongside:** 05's later steps. **Size:** L.

## Goal

One process that boots, routes, mixes, saves and shuts down in the right order.

## Read first

- [reference/host.md](../reference/host.md): the contract as lane 00 left it.
  `crates/vixmix-host/` already has the `Host` trait, `HostEvent`, `BackendState`,
  `MeterFrame` / `StatsFrame`, `RealHost` (boot, instances from the setup, the null backend
  pumped per poll, `apply` acting on `Applied`, `save_now` with the unreadable-file rule, editor
  sessions) and `FakeHost`. By the time this lane starts, 01–04 have replaced their skeletons.
- Every other reference ([model](../reference/model.md), [engine](../reference/engine.md),
  [pipewire](../reference/pipewire.md), [plugins](../reference/plugins.md),
  [app](../reference/app.md)) as shipped, and the handoffs' status lines for what each lane
  changed.
- [build-map.md](../build-map.md) § Change requests (request 1: wire the edit path the lanes
  agreed).
- The kit's [vst3-host.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/vst3-host.md)
  § Lending (teardown order: stop the preparing thread and drop its chains, then each instance,
  processor before controller) and
  [fs.md](https://github.com/thechimaeraproject/chimaeraproject-kit/blob/master/handbook/reference/fs.md).
- Vix's [docs/cookbook/by-hand-pass.md](https://github.com/thechimaeraproject/Vix/blob/master/docs/cookbook/by-hand-pass.md)
  (the shape of a by-hand cookbook).

## What this lane owns, what others keep

It owns the orchestration: boot and shutdown order, acting on `Applied` and on backend events,
routing apps to channels, the autosave, instance lifecycle, the null-backend fallback, and the
wiring in `app/src/main.rs` / `app.rs` that picks the host. Every other crate keeps its own
behaviour; the window keeps drawing (05).

## Steps

1. **`RealHost` boot:** load `setup.ron` and `plugins.ron`; create the instances with their
   state; `Engine::prepare`; `Backend::connect` on Linux, or the `NullBackend` elsewhere (with
   the banner); apply the `BackendGraph` derived from the setup.
2. **Routing:** on `AppStreamAdded`, match the app against the channels (`AppRef::matches`) and
   `route` it; an app no channel claims is returned to the default when it appears (the
   apply-on-appear rule, because WirePlumber restores what was written).
3. **On `SetupChanged`** decide re-prepare / re-apply the graph / re-route / save, from
   `Applied`; the save debounced 500 ms, atomic.
4. **Instance lifecycle** on insert add, remove and move (a move is the same instance, never a
   re-create); `SetInsertState` before every save and on removal, so an undo brings an insert
   back as it was; the host keeps a removed channel or insert until its undo toast dies.
5. **Every tick:** `Engine::reap` and `Instances::reap`; editors polled.
6. **Shutdown:** clear routings, `Backend::disconnect`, drop the graphs, retire the instances
   (processor before controller, on the UI thread), the final save.
7. **`FakeHost`** stays for the window's tests.
8. **A headless integration test:** the null backend plus the kit's test plugin through a
   channel chain with MON on; meters move; save → reload → the same setup.
9. **`cookbook/by-hand-pass.md`** for the first real test on Fedora: the user's Wave Link scene
   (Main Mic, Index Mic, Music, Web, System, Voice chat), with native Linux VST3s and
   yabridge-bridged Windows VST3s both appearing as bundles — with its index entry.
10. **Docs:** complete [reference/host.md](../reference/host.md); `CLAUDE.md` § Status;
    roadmap rows flipped.

## Files you own

`crates/vixmix-host/**`, the host wiring in `app/src/main.rs` and `app/src/app.rs` (agree any
other `app/` change with lane 05), `docs/reference/host.md`, `docs/cookbook/by-hand-pass.md`
(and its lines in `docs/cookbook/README.md` and the index), this handoff, your rows in
`docs/roadmap.md`, one paragraph in `CLAUDE.md` § Status.

## Acceptance

- `cargo run -p vix-mix` on Fedora (or the VM): a mic channel audible in headphones through a
  plugin; Spotify ticked into Music and heard in Monitor; Master recorded by another app from
  "Vix Mix Master"; quit and relaunch restores everything, plugin state included.
- The headless integration test above, on both legs.
- `reference/host.md` complete; `CLAUDE.md` § Status paragraph; roadmap rows flipped;
  `dev check` green.

## Out of scope

Packaging (08); the kit lifts (07); anything inside a crate another lane owns.
