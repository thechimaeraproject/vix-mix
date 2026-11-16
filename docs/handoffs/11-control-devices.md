# 11 — Devices over the control socket

**Record — shipped 2026-11-16.** The control socket of [handoff 10](10-control-socket.md)
now reads, sets and follows devices: the output a mix plays to (`/mix/<mix>/device/name`),
the input a hardware channel captures (`/ch/<id>/device/name`), each with its description
and whether it is there now, and the devices the machine has, listed under `/in` and `/out`.
A device is set by the two commands the window's own lists send, `SetMixDevice` and
`SetSource` with a hardware input, so a request now sends ten kinds of command. The contract
is [reference/control.md](../reference/control.md) § Addresses and § What a write sends; the
how is [cookbook/control-from-outside.md](../cookbook/control-from-outside.md) step 4; the
change to the shared contracts is the [build map](../build-map.md)'s change request 15,
applied. The protocol's number did not move: every address here is new.

**What was proven and what was not.** By tests, on every platform: a device set by its node
name, as it is listed and as it is described; the refusals; the list; a device unplugged told
to its followers; and, under the window's headless harness, a request for a device held to
the command the output panel and the source panel send. By hand on Fedora KDE against
`vix-mix --fake`: the list, Monitor switched twice with a follower printing both, Mic's input,
Master to none, and the three refusals ([reference/app.md](../reference/app.md)
§ Verification, By hand). **Not run: a real device.** The example console's devices are
names; nothing was heard move from one output to another, PipeWire was not given the graph a
mix's new output makes, and no device was pulled out of a real machine. The by-hand pass's
step 12 has that run, and it is the user's. `dev check` is green on Linux; the Windows leg
runs in CI.

Deviations from the brief below, each with its reason:

- **A device's values are a place, not one value**: `/mix/monitor/device/name`,
  `…/description` and `…/present`, as a `DeviceRef` is a name and a description. A deck key
  wants the description to show and the name to set, and `present` is the one fact the
  window's banner already tells about Monitor's device. `set /mix/monitor/device x` is
  refused with the three under it and the address to write.
- **The list is two places at the top, `/in` and `/out`**, not one `/devices`: a device
  that is unplugged is told as `gone`, and the protocol knows what went by an address's
  first two parts, which a device under `/devices/out/…` would not have.
- **A device not there cannot be set**, though the model would take any `DeviceRef`: a
  request carries a name, and the description a setup stores beside it comes from the
  host's list. The one exception is the device the setup already names, which may be named
  again while it is unplugged, so a controller that re-sends its state does not fail.
- **`Console`**: `leaves`, `plan`, `resolve` and `Service::publish` take a setup with the
  devices present, and a `&Setup` alone is still accepted as a console with none, so lane
  10's unit tests stand as they were written but for four expectations a device adds to.
  `tests/requests.rs`, which holds what a request may send, was rewritten for ten commands
  and a console with devices.
- **`Target::devices` has a default**, no devices, so a target that is a setup alone (the
  socket's and `ctl`'s tests) says nothing; the app's is the host's list, and
  `a_request_for_a_device_sends_the_command_its_panel_sends` fails without it.

## Goal

Switch what a mix plays to, and what a hardware channel captures, from outside: headphones
to speakers on a key, a deck key lit for the output in use. The user asked for it on
2026-11-16, the day the control socket shipped, as its follow-up.

Both changes are commands the window sends from its output and source panels
(`SetMixDevice`, `SetSource`), and the host already acts on them: a mix's new device is a new
graph for the backend ([reference/host.md](../reference/host.md) § Acting on a command). So,
as in lane 10, nothing is added to what the mixer does.

### Choices

- **A device is named by its PipeWire node name**, which is what the setup stores and what
  survives a restart and a replug; a person may write its description instead.
- **Only a hardware channel has an input.** An app channel's apps stay the window's, and a
  request never turns an app channel into a hardware one: `SetSource` is sent for a channel
  that is hardware already, with a hardware source.
- **The devices the machine has are readable and followable**, since a controller has to
  know what it may pick and when a device goes.

## Read first

- [handoff 10](10-control-socket.md) and [reference/control.md](../reference/control.md).
- [reference/model.md](../reference/model.md): `DeviceRef`, `DeviceInfo`, `Source`,
  `SetMixDevice` (Monitor keeps a device), `SetSource`.
- [reference/host.md](../reference/host.md): `Host::devices`, `HostEvent::Devices`, and what
  the host does with `Applied::graph`.
- `app/src/acceptance.rs`: `the_output_panel_picks_a_device_and_master_can_have_none` and
  `a_hardware_channel_picks_its_input`, the commands a request must match.

## What this lane owns, what others keep

| This lane | The others |
|---|---|
| the device addresses, how a device is written and which may be set, the list of devices and its `gone` | the model: what `SetMixDevice` and `SetSource` mean, and that Monitor keeps a device |
| that a request is held to the devices the host lists, and that followers hear the host's `Devices` event | the host: the list itself, the backend's graph for a new device, routing, the save |
| | the window: its output and source panels, an app channel's apps |

## Steps

1. **The console**: `Console { setup, devices }` in `vixmix-control`, taken by `leaves`,
   `plan`, `resolve` and `Service::publish`; `Target::devices`.
2. **The addresses**: a strip's `device/name`, `device/description`, `device/present`; `/in`
   and `/out`; a listed device resolved by its word, its node name or its description.
3. **The write**: `set` of a `device/name` to `SetMixDevice` or `SetSource`, the device
   found among the ones present, `none` for no device, every refusal ending with the
   devices there are.
4. **The app**: the host's devices behind the target, and `control::poll` publishing on
   `HostEvent::Devices` as on `SetupChanged`.
5. **The docs**: the references, the cookbook's step, the by-hand pass, change request 15.

## Files you own

`crates/vixmix-control/src/console.rs`, and what steps 1 to 3 name in `address.rs`,
`tree.rs`, `request.rs`, `service.rs` and `value.rs`; `app/src/control.rs`; the docs of
step 5.

## Acceptance

- `dev check` is green on both legs.
- The `match` over `Command` in `tests/requests.rs` names ten commands a request can send,
  and its property holds that a `SetSource` carries a hardware input only and that no
  request changes a channel's kind.
- A request for a mix's output and for a hardware channel's input sends the command the
  window's panel sends for the same pick, shown with `Recorder`.
- A device unplugged reaches a client that follows the list or the strip that uses it.

## Out of scope

An app channel's apps; the preferences; a device that is not there now; "the next device" as
one request; anything about what a device is beyond its name and description (its ports,
its rate).
