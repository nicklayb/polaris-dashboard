#!/usr/bin/env bash
# Opens the dashboard in a fixed-size floating foot window, using the same
# font as kmscon on the Pi (Cascadia Mono) so the character grid matches.
#
# Usage: run-sized.sh <WIDTHxHEIGHT> <font pixel height> <config>
set -euo pipefail

readonly window_class="polaris-preview"
readonly window_wait_attempts=50
readonly window_wait_interval=0.1

size="$1"
font_pixels="$2"
config="$3"

if [[ ! "$size" =~ ^([0-9]+)x([0-9]+)$ ]]; then
  echo "size must look like 1280x400, got '$size'" >&2
  exit 1
fi
width="${BASH_REMATCH[1]}"
height="${BASH_REMATCH[2]}"

# Shells inside tmux often miss the Hyprland and Wayland variables.
discover_hyprland_environment() {
  command -v hyprctl >/dev/null || return 0
  if [[ -z "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]]; then
    HYPRLAND_INSTANCE_SIGNATURE="$(hyprctl instances -j | jq -r '.[0].instance // empty')"
    export HYPRLAND_INSTANCE_SIGNATURE
  fi
  if [[ -z "${WAYLAND_DISPLAY:-}" ]]; then
    WAYLAND_DISPLAY="$(hyprctl instances -j | jq -r '.[0].wl_socket // empty')"
    export WAYLAND_DISPLAY
  fi
}

# Tiling would override the requested size: float, resize and center it.
pin_window_size() {
  local address=""
  for _ in $(seq "$window_wait_attempts"); do
    address="$(hyprctl clients -j | jq -r --arg class "$window_class" '.[] | select(.class == $class) | .address' | head -n1)"
    [[ -n "$address" ]] && break
    sleep "$window_wait_interval"
  done
  if [[ -z "$address" ]]; then
    echo "preview window not found, size left to the compositor" >&2
    return 0
  fi
  hyprctl --batch "dispatch setfloating address:$address; dispatch resizewindowpixel exact $width $height,address:$address; dispatch focuswindow address:$address; dispatch centerwindow" >/dev/null
}

cargo build --quiet --package polaris-tui
# Fail in the calling terminal rather than in a window that closes at once.
target/debug/polaris-tui --check "$config" >/dev/null
if [[ -z "${POLARIS_GRAFANA_TOKEN:-}" ]] && ! grep -qE '^\s*token_file\s*=' "$config"; then
  echo "warning: POLARIS_GRAFANA_TOKEN is not set (add it to .env) and $config has no token_file" >&2
fi
discover_hyprland_environment

FONTCONFIG_FILE="${POLARIS_FONTCONFIG:-${FONTCONFIG_FILE:-}}" foot \
  --app-id "$window_class" \
  --title "polaris ${size}" \
  --font "Cascadia Mono:pixelsize=${font_pixels}" \
  --window-size-pixels "${size}" \
  --override pad=0x0 \
  --hold \
  target/debug/polaris-tui "$config" &
terminal_pid=$!

if [[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]]; then
  pin_window_size
fi
wait "$terminal_pid"
