# Control Vix Mix from outside

Mute a channel from a key, turn a plugin off from a script, switch from headphones to
speakers, show a mute on a Stream Deck: every switch and fader of the console, and the
device each mix and hardware channel uses, can be read, written and followed by another
program of yours while Vix Mix runs. `vix-mix ctl` is the command for it, and under it is a Unix
socket that speaks one request a line. What each address reads and what a write sends is
[reference/control.md](../reference/control.md); the command and when the window serves it
are [reference/app.md](../reference/app.md) § The command line and § The control socket.

It changes what the console's own controls change and nothing else: no channel or plugin is
added, removed, moved or renamed from outside, and no app is moved between channels. Any program that runs as you may use it, as
any may run `wpctl`; nothing listens on the network.

To write a controller without touching the mixer that plays, run `vix-mix --fake` (the
example console, no audio) and put `--fake` after `ctl` in every command below; its socket is
`control-fake.sock` in the same directory.

## 1. See that the mixer answers

Start Vix Mix, then in a terminal:

```bash
vix-mix ctl get /mix/master/mute
```

It prints `/mix/master/mute false` (or `true`) and exits 0. An exit of 3 with "Vix Mix is not
running, or runs without its control socket" means what it says: start it. If Vix Mix is
running and a toast in its window said another Vix Mix holds the control socket, two are
running: close one.

The socket is `$XDG_RUNTIME_DIR/vix-mix/control.sock`, which on a desktop session is
`/run/user/<your uid>/vix-mix/control.sock`.

## 2. Read the console

```bash
vix-mix ctl get /
```

prints every value, one a line, as `address value`. The channels come first, left to right,
then Monitor and Master, then every plugin:

```
/ch/1/name "Mic"
/ch/1/mute false
/ch/1/monitor/mute false
/ch/1/monitor/db -3.43
/ch/1/monitor/position 0.76
…
/mix/master/preview false
/fx/2/name "Pro-C 2"
/fx/2/strip "/ch/1"
/fx/2/slot 2
/fx/2/enabled true
```

The number after `/ch` is the channel's id and the one after `/fx` is the plugin's. Neither
changes while the channel or the plugin exists, and neither is used again after it is removed,
so a program keeps them. A plugin dragged to another strip keeps its id; its `strip` and
`slot` say where it is. `get /ch`, `get /ch/1` and `get /fx/2` print a part.

By hand, write a channel by its name and a plugin by its strip and then its slot or its name:
`/ch/mic/mute`, `/ch/mic/fx/2/enabled`, `/ch/mic/fx/pro-c-2/enabled`,
`/mix/master/fx/1/enabled`. Case and punctuation do not matter (`voice-chat` is *Voice
chat*). The answer always names it by id.

## 3. Change a switch and a fader

```bash
vix-mix ctl toggle /ch/mic/mute              # both of Mic's sends
vix-mix ctl set /ch/mic/master/mute true     # one send
vix-mix ctl set /ch/music/monitor/db -12     # a fader, in decibels
vix-mix ctl nudge /mix/monitor/db -2         # two decibels down from where it is
vix-mix ctl set /ch/mic/fx/pro-c-2/enabled false
```

Each prints what it changed, as it reads now, and the window shows it at its next frame. A
channel's own `mute` is both its sends: it reads `true` when both are muted, and `toggle`
unmutes both when both are muted and mutes both otherwise. `db` is the fader in decibels,
from `-inf` (silent) to `6`; `position` is its travel from `0` to `1`, which is what a knob or
a slider on a controller should write, since the taper is the mixer's. On a channel whose
link is on, the other send moves with the one you set, as it does in the window.

A request the mixer refuses prints why on standard error, ending with what to send instead,
and exits 1.

## 4. Switch a device

```bash
vix-mix ctl get /out                                          # the outputs the machine has now
vix-mix ctl get /mix/monitor/device                           # the one Monitor plays to
vix-mix ctl set /mix/monitor/device/name Built-in speakers    # by its description
vix-mix ctl set /mix/monitor/device/name alsa_output.pci-0000_00_1f.3.analog-stereo
vix-mix ctl set /ch/mic/device/name "Yeti Stereo Microphone"  # a hardware channel's input
vix-mix ctl set /mix/master/device/name none                  # Master may have no device
```

`get /out` and `get /in` list every device the machine has now, a `name` and a `description`
each:

```
/out/alsa-output-pci-0000-00-1f-3-analog-stereo/name "alsa_output.pci-0000_00_1f.3.analog-stereo"
/out/alsa-output-pci-0000-00-1f-3-analog-stereo/description "Built-in Audio Analog Stereo"
```

Write a device by its `name` (PipeWire's node name, which stays the same across restarts and
replugging: what a program should keep) or by its `description` (what you read in the
window's output panel), in any case and with or without double quotes. A mix takes an output
and a hardware channel an input; an app channel has no device, and its apps are ticked in
the window. A device that is not plugged in cannot be picked, and the refusal lists the ones
that are. Monitor always keeps a device, so `none` is refused for it.

A mix's and a hardware channel's `device` has three values: `name`, `description`, and
`present`, which turns `false` when the device it is set to is unplugged.

## 5. Put a mute on a key

In KDE Plasma 6: *System Settings → Keyboard → Shortcuts → Add New → Command or Script…*.
Give it the command

```
vix-mix ctl toggle /ch/mic/mute
```

a name, and the key. Any desktop that runs a command on a key does the same. The command
takes a few milliseconds, and exits 3 without doing anything when Vix Mix is not running.

## 6. Follow a value from a script

```bash
vix-mix ctl watch /ch/mic/mute | while read -r address value; do
  notify-send "Mic" "muted: $value"
done
```

`watch` prints the value as it stands and then a line whenever it changes, whoever changed
it: the window, a key, another program. It goes on until it is stopped, or until Vix Mix
quits. `watch /ch/1` follows everything of a channel and `watch /` everything; a channel or a
plugin that is removed prints `/ch/3 gone`. It prints where a value stands, not every step: a
mute switched on and off again within a sixtieth of a second prints nothing.

## 7. Exchange lines from a program

A controller that shows state wants one connection it keeps. Two ways to have one:

**Start `vix-mix ctl -`** as a child and write requests to its standard input, a line each;
it writes what the mixer says to its standard output. When you close its standard input it
exits once everything was answered.

**Or open the socket** and do the same over it. In Python, with nothing installed:

```python
import os, socket

path = os.path.join(os.environ["XDG_RUNTIME_DIR"], "vix-mix", "control.sock")
mixer = socket.socket(socket.AF_UNIX)
mixer.connect(path)
lines = mixer.makefile("rw", buffering=1, encoding="utf-8", newline="\n")

print(lines.readline().strip())        # vix-mix control <the protocol's number>
lines.write("watch /ch\n")
for line in lines:
    line = line.rstrip("\n")
    if line.startswith("event "):
        address, value = line[len("event "):].split(" ", 1)
        ...                            # light a key, or put it out
```

What a program has to know:

- The mixer speaks first: one line, `vix-mix control ` and the protocol's number. The number
  moves when an address is renamed or a line changes its meaning, so refuse one you do not
  know.
- A request is one line, `verb address [value]`, with `get`, `set`, `toggle`, `nudge`,
  `watch` and `unwatch`. Its answer is zero or more `address value` lines and then exactly
  one line that is `ok` or begins `error: `.
- A line that begins `event ` is news for something you follow. It never comes inside an
  answer, but it can come between your request and its answer, so read lines one by one and
  sort them by how they begin.
- A value is `true` or `false`, a number, or text in double quotes as JSON writes it; any
  JSON reader reads a name. A silent fader's decibels are `-inf`.
- Read what you are sent. A connection that leaves 64 messages unread is closed. At most
  eight connections are served at once, and a ninth is told so on its first line.

## 8. Let a deck app in a Flatpak reach the socket

A sandboxed app sees neither the `vix-mix` command nor the socket. Give it the socket's
directory, and have its plugin open the socket as in step 7:

```bash
flatpak override --user --filesystem=xdg-run/vix-mix <the app's id>
```

Flatpak shares the directory as it finds it when the app starts, so start Vix Mix once
before the deck app; the directory then stays until you log out.

This step is written from Flatpak's documentation and has not been run with a deck app yet.
If it turns out wrong, fix it here in the commit that finds out.

## 9. How you know it worked

With Vix Mix running and a channel named Mic:

```bash
vix-mix ctl toggle /ch/mic/mute && vix-mix ctl get /ch/mic/mute
```

prints the values the toggle changed and then Mic's mute as it now is: `/ch/1/mute true` when
Mic was not muted (the id is your channel's). Both of Mic's mute buttons are lit in the
window, and Mic is silent in both mixes. Run it again and it reads `false`, the buttons go
out, and Mic is heard. The key from step 5 does the same with the window minimised. And

```bash
vix-mix ctl set /mix/monitor/device/name "<another output's description>"
```

moves what you hear to that output, with its radio picked in Monitor's output panel.
