# Nushell environment for new users.
#
# Prefer PATH management via systemd user environment.d (see
# `skel/.config/environment.d/10-distrobox-exports.conf`).
#
# If you *do* want to manage PATH in shell init, keep host “control plane”
# paths early, and avoid putting host toolchains (rustup/proto) ahead of
# `~/.local/bin/distrobox`.

# let extra_paths = [
#   ($env.HOME | path join ".local" "bin")
#   ($env.HOME | path join ".local" "bin" "distrobox")
#   ($env.HOME | path join "bin")
# ]
#
# let existing_extra = ($extra_paths | where {|p| ($p | path exists) })
#
# $env.PATH = ($env.PATH | prepend $existing_extra)

