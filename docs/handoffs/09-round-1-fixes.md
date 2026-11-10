# 09 — Round 1 fixes

**Record — in progress: the first pass ran on 2026-11-10 (handoff 08 step 4) and wrote the
Goal; its fixes are steps 5 and 6.** The pass was hours of daily use on Inochi (Fedora KDE, the
RPM built from commit `6c8bb3e`), not the cookbook step by step. This handoff also holds what
the earlier lanes left open for it, each numbered under [Open going in](#open-going-in) with
where it lives and what to check: one that held or was fixed is struck through with the date,
and the rest stay open, each under its issue. **Needs:** 08 (its step 4 is this handoff's first
step), 05. **Runs alongside:** 07. **Size:** set by the pass.

## Goal

What the first by-hand pass on the real machine found, fixed. The section is filled the way
Vix's
[handoff 22](https://github.com/thechimaeraproject/Vix/blob/master/docs/handoffs/22-round-1-fixes.md)
was filled: for each failure, what you did, what you saw and what you expected; then the steps
fix them, each in the crate that owns it. The pass of 2026-11-10 ran as use rather than in the
order of [the by-hand pass](../cookbook/by-hand-pass.md), so its two findings are in the order
they were met:

1. **A plugin that aborts while its module loads takes the scan, and Vix Mix, down with it**
   ([#26](https://github.com/thechimaeraproject/vix-mix/issues/26)). Did: Rescan plugins with
   Windows plugins bridged by yabridge, one of which (Softube's Saturation Knob) could not reach
   its licence service and returned no factory. Saw: yabridge's Linux side called
   `std::terminate` inside the module's entry, the process aborted, and the bundles after it
   were never reached. Expected: that bundle marked failed, the others scanned, Vix Mix still
   running. Step 5.
2. **The DSP readout flickers between about 12 % and 0 %**
   ([#25](https://github.com/thechimaeraproject/vix-mix/issues/25)). Did: one native plugin on a
   channel at a quantum of 256. Saw: the header's DSP figure alternating between about 12 % and
   0 %. Expected: a steady figure that still shows the expensive cycles. Step 6.

What the user saw hold in that pass is struck through below (items 6, 8, 11 and 12). The pass
measured nothing: the figures of items 1, 2 and 5 are still to take. Going in, the goal was the
fifteen items below.

## Read first

- [cookbook/install-on-fedora.md](../cookbook/install-on-fedora.md) and
  [cookbook/by-hand-pass.md](../cookbook/by-hand-pass.md): the procedure.
- [Handoff 08](08-packaging-and-first-real-test.md)'s status line: what it shipped and why its
  step 4 waited.
- [reference/host.md](../reference/host.md) § Every poll and § Shutdown,
  [reference/pipewire.md](../reference/pipewire.md) § Latency and § Verification,
  [reference/app.md](../reference/app.md) § What a frame draws.
- [Handoff 03](03-pipewire-backend.md)'s record (the two-minute run) and
  [handoff 06](06-host-and-integration.md)'s (the verifiers' notes).

## What this round owns, what others keep

Each step owns the crate it fixes and that crate's reference doc, as the lane that built the
crate did: `vixmix-host` with [reference/host.md](../reference/host.md), `vixmix-pipewire` with
[reference/pipewire.md](../reference/pipewire.md), the window in `app/` with
[reference/app.md](../reference/app.md), and the packaging files in `packaging/linux/` with
[install-on-fedora.md](../cookbook/install-on-fedora.md). A cookbook an item corrects is this
round's for that edit. Every other crate stays as it is. The shared contracts still change only
through a change request in [build-map.md](../build-map.md) § Change requests, agreed in the
main session, and a fix the kit owns (item 6) is a kit issue and a `kit bump`, never a patch
here.

## Steps

1. **Install** by [install-on-fedora.md](../cookbook/install-on-fedora.md) (step 4 or 5) and
   start Vix Mix from the KDE launcher: handoff 08's acceptance is that it installs by the
   documented path and appears in the launcher with its icon (item 11). Done 2026-11-10, from
   the RPM (step 5).
2. **Run [the by-hand pass](../cookbook/by-hand-pass.md)** ([#19](https://github.com/thechimaeraproject/vix-mix/issues/19) tracked it), steps 1 to 12 in one sitting,
   checking the items below where they come up. Write every failure into the Goal. Ran
   2026-11-10 as hours of daily use rather than in one sitting; #19 is closed.
3. **Record the figures**: the latency at 128, 256 and 512 and the `pw-top` ERR count at 256 in
   [reference/pipewire.md](../reference/pipewire.md) § Latency (item 2), and flip handoff 08's
   status line to say its step 4 ran. The status line is flipped; the figures were not taken
   on 2026-11-10 and stay open (items 1, 2 and 5).
4. **Write the steps**: one per finding, naming the crate and its reference; an item below that
   held is struck through with the date, one that did not becomes a step. Steps 5 and 6 are
   the two findings.
5. **Describe a bundle that must be loaded in a child process**
   ([#26](https://github.com/thechimaeraproject/vix-mix/issues/26)): `vixmix-plugins` with
   [reference/plugins.md](../reference/plugins.md) § Scanning and the cache, the host's boot
   ([reference/host.md](../reference/host.md) § Boot) and the app's `--scan-bundle`
   ([reference/app.md](../reference/app.md)). A crash, a refusal or a wait past the bound
   marks that one bundle failed, the scan goes on, and the scan cache keeps the failure as it
   keeps any other. Loading a plugin into a channel stays in process. Built 2026-11-10:
   `Catalog::rescan_with` and `vix-mix --scan-bundle` (change request 13); a bundle with a
   readable `moduleinfo.json` is still described in process, since that runs none of its code.
   Its tests load a test plugin that aborts and one that never answers through a real child.
   Still to do by hand on Fedora: Rescan plugins with the plugin that aborted (Saturation Knob
   through yabridge, taken off yabridge's blacklist): Vix Mix stays up, the toast names the
   bundle, and the plugins after it are found.
6. **A DSP load that reads steadily**
   ([#25](https://github.com/thechimaeraproject/vix-mix/issues/25)): `vixmix-engine`
   ([reference/engine.md](../reference/engine.md) § Meters and stats), `vixmix-host` and the
   header's display in the app. The figure is the busy time over the budget across a short
   window, and the most expensive cycle stays visible as a peak. Built 2026-11-10 (change
   request 12): the tick keeps its busy time and its worst cycle, the host reads the load over
   250 ms and holds the worst cycle for a second, and the header shows the average with a peak
   tick on its bar, tenths below 1 %, and the number in the warning colour while a cycle is
   over its budget. The look gate's reference capture was retaken for the tick
   ([reference/app.md](../reference/app.md) § The capture). Still to do by hand on Fedora: the
   plugin that showed it, at a quantum of 256, reads a steady figure.

## Open going in

1. **The two-minute run at 256 without an xrun, with realtime scheduling.** Where:
   `two_minutes_at_256_without_an_xrun` in `crates/vixmix-pipewire/tests/headless.rs`;
   [reference/pipewire.md](../reference/pipewire.md) § Verification; handoff 03's record. The
   container passed it every run, but a container has no realtime budget, so handoff 03's
   acceptance stays open until a shell with realtime scheduling runs it. Check: on Fedora, run
   [Run a headless PipeWire](../cookbook/run-headless-pipewire.md) steps 3 and 6 against a
   daemon of its own, never the desktop's (the tests write routings WirePlumber remembers), in a
   shell started as below, which keeps its socket, its session bus, its drop-in and
   WirePlumber's state off the desktop's; PipeWire asks RTKit for the data thread's priority
   there. Zero ERR on every node of ours over the window meets the acceptance; record the run
   in pipewire.md § Verification.

   ```bash
   env -u XDG_RUNTIME_DIR -u DBUS_SESSION_BUS_ADDRESS \
     XDG_CONFIG_HOME="$(mktemp -d)" XDG_STATE_HOME="$(mktemp -d)" bash
   ```

2. **The latency and the xruns at 256 on real devices.** Where: the by-hand pass steps 10 and
   11; [reference/pipewire.md](../reference/pipewire.md) § Latency, beside the estimate (about
   three quanta, 16 ms at 256). Check: the three `jack_iodelay` figures, the date, the interface
   and the kernel, and the ERR count the `vix-mix.*` rows of `pw-top` reached in five minutes at
   256, with what was happening when one moved. Seen 2026-11-10, not measured: hours at a
   quantum of 256 with the header's xrun count not moving.
3. **Vix Mix started again within milliseconds of quitting can leave a playing app routed but
   silent.** ([#13](https://github.com/thechimaeraproject/vix-mix/issues/13)) Seen with
   `pw-play` in the container: the player kept playing through the quit and was moving back to
   the default device when a mixer booted again at once routed it into its channel, while the
   graph was still moving to the player's own quantum; the player's node then erred every cycle
   and went away. A mixer started a second later did not break it, so
   `crates/vixmix-host/tests/headless.rs` waits for the player to settle before it boots again,
   and the by-hand pass step 9 asks you to note how soon after the quit you started Vix Mix
   (its "a player restarted into its channel" is that player, routed back by the mixer started
   again). Check: in step 9, quit Vix Mix and start it again at once while Spotify plays,
   several times. If Spotify ends silent, shutdown should wait, bounded, until each app it
   routed home has left our sinks before `Backend::disconnect`
   ([reference/host.md](../reference/host.md) § Shutdown; `vixmix-host`, with the backend
   reporting the move), and step 9's sentence should say the mixer, not the player, started
   again.
4. ~~**Monitor's device gone, with Master on "No device".**~~ **Fixed 2026-11-10**
   ([#14](https://github.com/thechimaeraproject/vix-mix/issues/14)): while the Master source
   has the tick and the device the setup names for Monitor is not among the devices the backend
   reports, a toast (once for that device) and the idle banner name the device and say the
   mixer waits for it; the banner while nothing drives the mixer names it too
   ([reference/host.md](../reference/host.md) § Every poll and the state on PipeWire). Its
   tests compile only on Linux, so CI's Linux leg proves them. Narrower than the issue asked,
   on purpose: Monitor's stream refused on a device that is there now says nothing, where it
   said "is not there" before, which was false and could repeat each second as the backend
   tried again. Still to do by hand on Fedora: the check below. Was: where,
   `crates/vixmix-host/src/audio.rs`. The toast "Monitor's output device is not there…" is said
   only when Master out takes the tick; with Master on "No device" the tick moves to the Master
   source instead and nothing is said, and the idle banner asks to pick a Monitor device
   although one is named and merely absent. Check: with step 3's outputs (headphones on
   Monitor, Master on "No device"), unplug the headphones: the window should name the missing
   device and say it waits for it; plug them back and Monitor plays again.
5. **The click at a quantum change.** The by-hand pass step 10 says one short click at each
   change; measured, the click comes only when the quantum grows, it counts as one xrun in the
   mixer's stats, and the ERR of the `vix-mix.*` rows in `pw-top` does not move. Check it with
   real devices (256 to 512: one click and the header's xrun count up one; 512 to 256: none),
   then make the by-hand pass step 10, the `graph` bullet of
   [reference/host.md](../reference/host.md) § Acting on a command, and
   [reference/pipewire.md](../reference/pipewire.md) § Latency and § Verification say so.
6. ~~**`/usr/lib64/vst3`.**~~ **Held 2026-11-10** (the user's pass: packaged plugins appeared
   in the browser with no folder added). The kit's `default_search_dirs` now lists it, where
   Fedora's packages install VST3 plugins
   ([kit issue 201](https://github.com/thechimaeraproject/chimaeraproject-kit/issues/201), taken
   with handoff 07's bump; the workaround lines are gone from the by-hand pass step 1 and
   install-on-fedora.md step 6). Check: `rpm -qla | grep -F .vst3/ | cut -d/ -f1-5 | sort -u`
   names bundles under `/usr/lib64/vst3`, and their plugins appear in the browser with no folder
   added.
7. **The first start.** ([#16](https://github.com/thechimaeraproject/vix-mix/issues/16)) The
   scan of every VST3 bundle runs before the window opens, with nothing on screen
   ([reference/host.md](../reference/host.md) § Boot, step 2), and Monitor's device is not
   picked on a first start (the banner asks). Check: how long the first start takes with the
   machine's plugins and whether the wait reads as a hang; whether Monitor should start on the
   default output. Both are the user's call; a progress display needs the scan off the window's
   first frame (`vixmix-host` and the app).
8. ~~**What the window shows for the backend's states.**~~ **Held 2026-11-10** for the states
   the user's pass met (each said what to do); the gap already known is item 4. The banner says
   the backend's state (PipeWire's refusal, the mixer idle with what would drive it, the daemon gone), the host's
   notices and refusals go to the toast line, and an insert playing as a pass-through is marked
   on its slot with the host's why under the pointer (`app/src/ui.rs`, `app/src/toast.rs`,
   `app/src/well.rs`; [reference/app.md](../reference/app.md) § What a frame draws). Check each
   state on the real desktop, and that each says what to do: connecting (the start), idle with
   nothing to drive it (no output on either mix and nothing recording "Vix Mix Master"), idle
   with only the Master source (an app recording it starts the mixer), running, the daemon gone
   (`systemctl --user restart pipewire pipewire-pulse wireplumber`), Monitor's device gone, a
   plugin missing (a bundle renamed), a scan failure.
9. **The window's entry in the task manager, and the launch feedback.**
   ([#18](https://github.com/thechimaeraproject/vix-mix/issues/18)) Where:
   `packaging/linux/org.chimaeraproject.vixmix.desktop` and `app/` (the window, lane 05). winit
   gives an X11 window the class `vix-mix`, the binary's name, which the desktop entry's
   `StartupWMClass` matches; on Wayland it sets no app id unless the app names one, so Plasma
   guesses from the process. The entry leaves `StartupNotify` out, because the app does not yet
   read the activation token the launcher hands it (`XDG_ACTIVATION_TOKEN` on Wayland,
   `DESKTOP_STARTUP_ID` on X11): `true` would promise it does, `false` would turn the feedback
   off where `StartupWMClass` ends it. Check: the running window's task-manager entry and title
   bar show the icon, on XWayland, where the window runs under KDE's Wayland session (its event
   loop is X11 whenever `DISPLAY` is set, handoff 05), and on Wayland only with no X display;
   and after a start from the launcher, whether the bouncing cursor and the task manager's
   launch entry go when the window appears or stay until Plasma's timeout. If the icon is
   generic, the app names its window with winit's `with_name` (the app id
   `org.chimaeraproject.vixmix` on Wayland, the class `vix-mix` on X11). If the feedback stays,
   the app reads the token with winit's `EventLoopExtStartupNotify::read_token_from_env`, makes
   its window `with_activation_token` (`WindowAttributesExtStartupNotify`), calls
   `reset_activation_token_env` so no child process inherits it, and the desktop entry then
   says `StartupNotify=true`. Seen 2026-11-10: the icon reads in the launcher and the task
   manager; the launch feedback was not checked.
10. **Logging out with apps routed.**
    ([#12](https://github.com/thechimaeraproject/vix-mix/issues/12)) Where: `app/src/main.rs`
    (no signal handling) and [reference/host.md](../reference/host.md) § Shutdown. A window
    asked to close (Plasma's logout should ask) runs the shutdown order; a `SIGTERM` (`pkill
    vix-mix`) ends the process without it, as `pkill -9` does in the by-hand pass step 9.
    Check: log out with Spotify in Music and log in again: the setup as it was, Spotify on the
    default output; then the same after `pkill vix-mix`. If an app is left on a sink that is
    gone, or the setup lost a change older than a second, the app takes `SIGTERM` as a close
    request.
11. ~~**The install itself.**~~ **Held 2026-11-10 for the RPM**: installed with `dnf` from the
    build's artifact (install-on-fedora.md step 5), started from the launcher, the icon reading
    in the launcher and the task manager; the RPM's own build runs `desktop-file-validate` and
    `appstream-util validate-relax` with Fedora's versions. Not checked, because the pass built
    nothing on the machine: that step 1's `dnf` list is all a build from source needs (a
    package the build still asks for is a finding). Where: `packaging/linux/` and
    [install-on-fedora.md](../cookbook/install-on-fedora.md), built and validated only in the
    Ubuntu container before (handoff 08's record).
12. ~~**The by-hand pass starts from the launcher.**~~ **Done 2026-11-10**: with item 11 held,
    the by-hand pass steps 1 and 2 point at install-on-fedora.md for the install and start Vix
    Mix from the launcher rather than repeating the `dnf` list and
    `cargo run --release -p vix-mix`, so every later pass proves the install too.
13. **A frame for the meters alone.**
    ([#17](https://github.com/thechimaeraproject/vix-mix/issues/17)) Where: `app/src/app.rs`
    and `app/src/ui.rs`; [reference/app.md](../reference/app.md) § Verification (the CPU
    bullet) and [handoff 05](05-app.md)'s CPU deviation. The main session ruled handoff 05's
    "60 fps under 5 % CPU" per machine, met at 0.16 to 0.46 % on the 24-core Windows machine;
    of one core the window takes 3.8 to 6.3 % in a release build and 5.5 to 11 % in a debug one
    at 60 frames a second. Build: when nothing but the meters changed since the last frame,
    draw the meters over the console's last tessellation and skip egui's pass (about 0.6 ms of
    the debug build's 1.1 ms frame), a second drawing path that must know everything that can
    change. Check: the per-core figure, measured as the reference measures it, under 5 % in a
    release build on Windows and on Fedora, and `--dump-png` still byte for byte the look
    gate's.
14. ~~**Change request 10.**~~ **Fixed 2026-11-10**
    ([#11](https://github.com/thechimaeraproject/vix-mix/issues/11)): `Host::resize_editor`
    is on both hosts and the editor windows call it when a window says it took a new size
    (never a size with no area, one the window has left, or the one its plugin already has, so
    nothing goes round); the bound of sixteen extra plugin folders is the model's
    (`Prefs::MAX_PLUGIN_DIRS`), refused by `SetPrefs` and at load with the fix. Tests pin both;
    the references are [host.md](../reference/host.md), [model.md](../reference/model.md) and
    [app.md](../reference/app.md). Still to do by hand, on Windows and on Fedora: an editor
    whose plugin asks for a new size draws at that size with no strip of the old one. Was:
    where, `crates/vixmix-host` (`Host::resize_editor` in both hosts, `FakeHost`'s doing
    nothing), `crates/vixmix-model` (`Command::SetPrefs`) and `app/src/editors.rs` (the call,
    on the editor window's `Resized`); [build-map.md](../build-map.md) § Change requests, row
    10, agreed 2026-11-04. Build: the host tells an editor's plugin the size its window took
    (`EditorSession::resize`, which the kit does asynchronously), and the model refuses a
    seventeenth extra plugin folder with the window's words (remove one first), as
    `popover::MAX_PLUGIN_DIRS` holds it today. Check: an editor whose plugin asks for a new
    size draws at that size with no strip of the old one, on Windows and on Fedora; a
    seventeenth folder, sent by any means, is refused with the fix.

15. ~~**Preferences: the Tab order of the folder field and its Add button.**~~ **Fixed
    2026-11-10** ([#15](https://github.com/thechimaeraproject/vix-mix/issues/15)): the field is
    registered before the button, Tab goes from the field to Add and Shift+Tab back, Add shows
    the focus ring when Tab reaches it, and nothing moved on screen; a test pins the order.
    Was: where, `app/src/popover.rs`, `folder_field` registers the Add button before the folder text field,
    so Tab from the field goes to Done and Shift+Tab goes back to Add, the reverse of the
    visual order (the field, then Add). Fix: lay the field out before the button, and pin it
    with a test (Tab from the folder field focuses the add-folder button). Polish; it predates
    lane 05's closing round.

## Files you own

Set by the pass: each step names the crate it fixes and the reference it updates, and those
crates' lanes have shipped, so this round's steps own those files. Going in: this handoff, the
cookbooks and references the items name, and the measured figures in
[reference/pipewire.md](../reference/pipewire.md).

## Acceptance

- The by-hand pass has run on Fedora KDE and every failure is in the Goal with what was done,
  seen and expected.
- Every item above is struck as held, with the date, or fixed by a step with a test where a
  test can pin it.
- The by-hand pass's failed steps pass on a second run; `dev check` is green; handoff 08's
  status line says its step 4 ran.

## Out of scope

New features: the Windows backend, plugin isolation, the RPM spec and a COPR
([roadmap](../roadmap.md) § Named but not built).
