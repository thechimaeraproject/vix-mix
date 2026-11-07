# Run the by-hand pass

The first real test of Vix Mix, on the Fedora KDE machine (or the Fedora VM): the user's Wave
Link scene built on real devices and real apps, checked by ear and with PipeWire's own tools.
What the automated tests cannot hear — a mic through a plugin in headphones, Spotify coming back
to its channel, Discord and OBS recording "Vix Mix Master", xruns on a desktop under load, the
latency of the real devices — only a person at the machine finds. Every step says what to do and
what must happen; a step *passes* when it does. [Handoff 08](../handoffs/08-packaging-and-first-real-test.md)
runs it; what fails goes into the next handoff's goal with what you did, what you saw and what
you expected.

What the host does behind each step is [reference/host.md](../reference/host.md); the nodes,
the routing and the latency budget are [reference/pipewire.md](../reference/pipewire.md). The
window's controls (the source and mix panels, the plugin browser, editor windows) are
[handoff 05](../handoffs/05-app.md)'s steps 3–7, so run this pass once they have shipped.

## 1. Get the machine ready

PipeWire and WirePlumber are Fedora's default; check they run and note the default output:

```bash
systemctl --user status pipewire wireplumber
pw-metadata -n default 0 default.audio.sink
```

Install what the build and the checks need, and Rust through rustup (the repository's
`rust-toolchain.toml` picks the version):

```bash
sudo dnf install pipewire-devel clang-devel pipewire-utils pipewire-jack-audio-connection-kit \
  jack-example-tools git gcc
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

The plugins: FabFilter, iZotope and Kilohearts have no Linux builds, so the pass uses native
Linux VST3s (TDR, u-he, werman, Airwindows, Surge XT, Dragonfly, LSP, ZL) and Windows VST3s
bridged by yabridge (`yabridgectl add <the Wine VST3 folder>`, then `yabridgectl sync`), both
appearing as `.vst3` bundles. Vix Mix looks in `~/.vst3` (yabridge's `~/.vst3/yabridge`
included), `/usr/lib/vst3`, `/usr/lib64/vst3` (where Fedora's packages install VST3s) and
`/usr/local/lib/vst3`; a plugin anywhere else is a folder to add to the preferences' plugin
directories, then rescan (step 4).

## 2. Build and start Vix Mix

The kit's crates come from its private repository through the git CLI, with this machine's
credentials (`gh auth login` first if `git ls-remote` on the kit asks for a password).

```bash
git clone https://github.com/thechimaeraproject/vix-mix && cd vix-mix
cargo run --release -p vix-mix
```

The window opens titled "Vix Mix". A first start scans every VST3 bundle before the window
appears, which takes a while with many plugins; later starts reuse `~/.config/vix-mix/plugins.ron`
and are quick. A release build keeps its setup in `~/.config/vix-mix/` (a debug build in
`~/.config/vix-mix/profiles/dev/`). On a first start there is one channel, Mic, with no device,
and neither mix has an output, so the banner says the mixer runs only while an app records
"Vix Mix Master" and asks for an output for Monitor. `pw-cli ls Node | grep vix-mix` lists
`vix-mix.master`, the virtual microphone; a hardware channel has its node once its device is
picked.

## 3. Pick the outputs

In Monitor's panel pick the headphones; in Master's pick "No device" (Master is still a virtual
microphone). The banner goes; the header shows `48 kHz`, `256 samples`, `5.3 ms` and the DSP
load. `pw-link -l` shows `vix-mix.monitor-out` linked to the headphones.

## 4. Build the Wave Link scene

Make the user's scene, each channel from the add-channel column, named in its source panel:

| Channel | Source |
|---|---|
| Main Mic | Hardware: the main microphone (rename Mic) |
| Index Mic | Hardware: the second microphone |
| Music | Apps: Spotify |
| Web | Apps: Firefox (and Chrome, if used) |
| System | Apps: the desktop's own sounds (play a notification: the source panel lists what plays it) |
| Voice chat | Apps: Discord |

An app is ticked in one channel only: ticking it in another takes it out of the first. Each
channel is a node named after it: `pw-cli ls Node | grep vix-mix.ch` lists
`vix-mix.ch1.main-mic`, `vix-mix.ch2.index-mic`, `vix-mix.ch3.music`, `vix-mix.ch4.web`,
`vix-mix.ch5.system` and `vix-mix.ch6.voice-chat` (the numbers are the channels' ids, so they
follow the order the channels were made). In Voice chat, mute the Master send, so the people in
the call do not hear themselves come back. If a plugin you installed is missing, add its folder
in the preferences and use the menu's Rescan plugins: it appears in the browser, and an insert
that was waiting for it starts playing.

## 5. A mic through a plugin to the headphones

On Main Mic, add a native plugin (TDR Nova, say) and then a bridged one; MON lit. Talk: your
voice is in the headphones through both, and Main Mic's Monitor meter moves. Open each plugin's
editor from its slot, move a control that is easy to hear (Nova's gain): the sound follows at
once. Toggle a slot's bypass square: the plugin drops out and comes back. Remove a plugin and
press Undo on the toast within five seconds: it comes back in its slot with its settings (the
same instance, never made again). Turn MON off: the headphones hear the dry mic.

## 6. Spotify into Music, across its restarts

Tick Spotify in Music and play a song: Music's meters move, and the song is in the headphones.
`pw-link -l | grep -A2 -i spotify` shows it linked to `vix-mix.ch3.music`. Quit Spotify
completely (its tray icon too), start it again and play: it comes back in Music by itself — the
host routes every app the moment it appears, to its channel, whatever WirePlumber remembered.
Do the same with Firefox playing a video: it comes back in Web. An app no channel names plays to
the default output as it would without Vix Mix.

## 7. "Vix Mix Master" in Discord and OBS

Discord: Settings, Voice & Video, input device "Vix Mix Master"; use the mic test and talk: its
meter moves, and you hear what Master carries (your voice, the music) and not the call itself
(step 4 muted Voice chat's Master send). OBS: add an Audio Input Capture source on "Vix Mix
Master": its meter in the audio mixer moves with Master's. Record ten seconds in OBS and play the
file: the voice and the music, nothing doubled. `pw-link -l | grep -B1 vix-mix.master` shows
both recording it.

## 8. Master previewed through Monitor

Press the ear on Master: Monitor plays the Master mix (its strip shows the MST tag), so the
headphones hear what Discord and OBS hear. Mute Music's Master send: the music goes from the
headphones and from OBS's meter. Unmute it and press the ear again: Monitor is its own mix again.

## 9. Save and restore, plugin state included

Change a plugin's settings in its editor so they are easy to recognise, wait a second (the setup
is saved half a second after the last change), and close the window. With Spotify still
playing, it goes back to the default output within a moment: Vix Mix clears its routings when
it quits. `~/.config/vix-mix/setup.ron` holds the six channels, their sources and the plugins'
states. Once Spotify plays on the default output, start Vix Mix again: the same channels,
faders, mutes and devices, Spotify back in Music, and the plugin sounding as it did, its editor
showing the settings you made. Then change a setting and close the window at once, without
waiting: the quit's own save keeps it, and the next start sounds so. Kill it instead
(`pkill -9 vix-mix`) a second or more after a change and start it again: the change is there.
If Spotify is silent after a start, write down how soon after the quit you started it: in the
headless rig a player restarted into its channel within milliseconds of the quit broke, and one
a second later did not.

## 10. No xruns at 256

With music playing through Music, the mic through its plugins, and Discord and OBS recording
Master, run:

```bash
pw-top
```

For five minutes, while the desktop is used normally (a browser, a game), the `vix-mix.*` rows
stay at QUANT 256 and their ERR column does not move. Then set the preferences' quantum to 512
and back to 256 while everything plays: the header follows, Spotify stays in Music, Discord and
OBS go on recording "Vix Mix Master" (a quantum change keeps every node of Vix Mix, so nothing
has to link again), and the only trace is one short click at each change. The header's DSP load stays well under
its bar's end. Write down any ERR count that moved, and what was happening at the time.

## 11. Measure the latency

The figure handoff 03 estimates (about three quanta, 16 ms at 256, app to audible) is measured
here, with a loopback cable from the audio interface's output to one of its inputs and
`jack_iodelay`, which sends a signal and times its return:

```bash
pw-jack jack_iodelay
```

Pick the interface's output for Monitor and mute Web's Master send. Link `jack_iodelay`'s output
into Web's sink and the interface's input, where the cable returns, straight to `jack_iodelay`'s
input (`pw-link -o` and `pw-link -i` list the port names):

```bash
pw-link 'jack_iodelay:out' 'vix-mix.ch4.web:playback_FL'
pw-link '<the interface capture port>' 'jack_iodelay:in'
```

`jack_iodelay` prints the round trip in frames and milliseconds: an app's signal → Vix Mix (one
quantum in the ring) → the interface's output → the cable → its input. Read it at 256, then set
the preferences' quantum to 128 and to 512 and read it at each once the header shows the new
quantum and the figure has settled: the two links stay through the change, because a quantum
change keeps every node of Vix Mix (if `pw-link -l` no longer shows them, make them again and
write that down as a finding). Write the three figures, the date, the interface and the kernel
into [reference/pipewire.md](../reference/pipewire.md) § Latency, beside the estimate. Unplug
the cable before stopping `jack_iodelay`.

## 12. How you know it worked

Every expectation above held on the Fedora machine in one sitting, and reference/pipewire.md
§ Latency carries the measured figures. Then run, in the checkout:

```bash
dev check
```

and it ends with every gate passing. Anything that did not hold is a finding: write it into the
next handoff's goal with what you did, what you saw and what you expected, and leave this
cookbook's steps as they are until the fix lands — a step that describes the fix before it ships
is a dead note.
