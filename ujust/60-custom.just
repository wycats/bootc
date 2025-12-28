# Custom ujust recipes for this bootc image.
#
# This file is auto-imported by ublue-os-just (as an optional import).
# Location in the image: /usr/share/ublue-os/just/60-custom.just

set allow-duplicate-recipes := true

# Re-apply our first-login bootstrap (Flatpaks + GNOME extensions)
bootc-bootstrap:
    /usr/local/bin/bootc-bootstrap apply

# Force the bootstrap to run again on next login by clearing its state file
bootc-bootstrap-reset:
    rm -f "${XDG_STATE_HOME:-$HOME/.local/state}/bootc-bootstrap/last-applied.sha256"
    echo "bootc-bootstrap state cleared"
