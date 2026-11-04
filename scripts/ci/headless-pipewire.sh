#!/usr/bin/env bash
# Starts a headless PipeWire and WirePlumber with a null sink and a null source, and proves the
# daemon answers: CI's Linux leg runs it before the gates (.github/workflows/ci.yml), and it
# works the same in a WSL2 or VM shell with no audio session. Handoff 03's integration tests
# run against what it starts (docs/handoffs/03-pipewire-backend.md).
#
# Needs the packages CI installs: pipewire, wireplumber, pipewire-audio-client-libraries
# (pw-cli). The daemons are left running in the background; their logs are in $LOGS (default
# the runner's temp directory, else /tmp). In GitHub Actions the variables later steps need
# are appended to $GITHUB_ENV; in a shell, `source` this script to keep them. Sourced, it
# restores your shell's options at the end and returns on failure instead of closing the shell.
_vixmix_pw_sourced=0
if [ "${BASH_SOURCE[0]}" != "$0" ]; then _vixmix_pw_sourced=1; fi
_vixmix_pw_options="$(set +o)"
set -euo pipefail

LOGS="${LOGS:-${RUNNER_TEMP:-/tmp}}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-$LOGS/vixmix-xdg-runtime}"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"

# A drop-in, so the distribution's pipewire.conf (its Dummy-Driver among it) stays as it is.
conf="${XDG_CONFIG_HOME:-$HOME/.config}/pipewire/pipewire.conf.d"
mkdir -p "$conf"
cat >"$conf/90-vixmix-null-devices.conf" <<'CONF'
# vix-mix CI: one null output and one null input for the backend's tests (scripts/ci/headless-pipewire.sh).
context.objects = [
  { factory = adapter
    args = {
      factory.name     = support.null-audio-sink
      node.name        = "vixmix-ci-null-sink"
      node.description = "vix-mix CI null sink"
      media.class      = "Audio/Sink"
      audio.position   = [ FL FR ]
      object.linger    = true
      monitor.channel-volumes = true
    }
  }
  { factory = adapter
    args = {
      factory.name     = support.null-audio-sink
      node.name        = "vixmix-ci-null-source"
      node.description = "vix-mix CI null source"
      media.class      = "Audio/Source/Virtual"
      audio.position   = [ FL FR ]
      object.linger    = true
    }
  }
]
CONF

# WirePlumber talks D-Bus; a runner has no session bus, so start one.
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && command -v dbus-daemon >/dev/null 2>&1; then
  if _vixmix_pw_bus="$(dbus-daemon --session --fork --print-address)"; then
    export DBUS_SESSION_BUS_ADDRESS="$_vixmix_pw_bus"
  fi
fi

pipewire >"$LOGS/pipewire.log" 2>&1 &
for _ in $(seq 1 50); do
  [ -S "$XDG_RUNTIME_DIR/pipewire-0" ] && break
  sleep 0.2
done
wireplumber >"$LOGS/wireplumber.log" 2>&1 &

# The proof: the null sink is a node, and WirePlumber has made the `default` metadata the
# backend routes through.
answered=""
for _ in $(seq 1 75); do
  if pw-cli ls Node 2>/dev/null | grep -q '"vixmix-ci-null-sink"' &&
    pw-cli ls Metadata 2>/dev/null | grep -q '"default"'; then
    answered=yes
    break
  fi
  sleep 0.2
done
pw-cli ls Node | grep -E 'node.name|media.class' || true
if [ -z "$answered" ]; then
  echo "headless-pipewire: the daemon did not list vixmix-ci-null-sink and the default metadata within 15 s" >&2
  echo "--- pipewire.log" >&2; cat "$LOGS/pipewire.log" >&2 || true
  echo "--- wireplumber.log" >&2; cat "$LOGS/wireplumber.log" >&2 || true
  eval "$_vixmix_pw_options"
  if [ "$_vixmix_pw_sourced" = 1 ]; then return 1; fi
  exit 1
fi
echo "headless-pipewire: the daemon answers at $XDG_RUNTIME_DIR/pipewire-0 with the null sink, the null source and the default metadata"

# The backend's integration tests (crates/vixmix-pipewire/tests/headless.rs) run only where this
# marker says the daemon is this script's, never against a desktop session they could disturb.
export VIXMIX_HEADLESS_PIPEWIRE=1

if [ -n "${GITHUB_ENV:-}" ]; then
  echo "XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR" >>"$GITHUB_ENV"
  echo "VIXMIX_HEADLESS_PIPEWIRE=1" >>"$GITHUB_ENV"
  if [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    echo "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS" >>"$GITHUB_ENV"
  fi
fi

eval "$_vixmix_pw_options"
