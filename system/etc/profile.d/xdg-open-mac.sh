#!/bin/bash
# xdg-open forwarder for macOS VM host
#
# When running in a VM with a configured Mac host, this wraps xdg-open to
# forward URL opens to macOS via SSH. This enables OAuth flows (VS Code
# GitHub login, etc.) to open in the Mac's default browser.
#
# Configuration: ~/.config/bootc/mac-host
#   Contains the SSH destination for the Mac host (e.g., wycats@192.168.0.36)
#   If this file doesn't exist, xdg-open behaves normally.

xdg-open() {
  local mac_host_file="${XDG_CONFIG_HOME:-$HOME/.config}/bootc/mac-host"

  # If no mac-host configured, use real xdg-open
  if [[ ! -f "$mac_host_file" ]]; then
    command xdg-open "$@"
    return $?
  fi

  local mac_host
  mac_host="$(<"$mac_host_file")"

  # Only forward URLs (http/https), not local files
  if [[ "${1:-}" =~ ^https?:// ]]; then
    ssh -o ConnectTimeout=3 -o BatchMode=yes "$mac_host" "open '$1'" 2>/dev/null
    return $?
  fi

  # For non-URLs, use real xdg-open
  command xdg-open "$@"
}
