# Custom ujust recipes for this bootc image.
#
# This file is auto-imported by ublue-os-just (as an optional import).
# Location in the image: /usr/share/ublue-os/just/60-custom.just

# Re-apply our first-login bootstrap (Flatpaks + GNOME extensions)
bootc-bootstrap:
    /usr/bin/bootc-bootstrap apply

# Force the bootstrap to run again on next login by clearing its state file
bootc-bootstrap-reset:
    rm -f "${XDG_STATE_HOME:-$HOME/.local/state}/bootc-bootstrap/last-applied.sha256"
    echo "bootc-bootstrap state cleared"

# Enable Steam Remote Play style 'console mode' (Option B): gamescope DRM + Steam gamepad UI on tty2.
# This only installs + enables the unit; it does not start it.
enable-remote-play:
    @bash -euo pipefail -c '
        host() {
            if command -v flatpak-spawn >/dev/null 2>&1 && ([ -f /.containerenv ] || [ -f /run/.containerenv ]); then
                flatpak-spawn --host "$@"
            else
                "$@"
            fi
        }

        user="${SUDO_USER:-$USER}"
        echo "Enabling remote play for user: ${user}"
        host sudo install -Dpm0755 /usr/share/bootc-optional/remote-play/bin/bootc-remote-play-tty2 /usr/bin/bootc-remote-play-tty2
        host sudo install -Dpm0644 /usr/share/bootc-optional/remote-play/systemd/bootc-remote-play-tty2@.service /etc/systemd/system/bootc-remote-play-tty2@.service
        host sudo install -Dpm0644 /dev/null /etc/bootc-remote-play/enabled
        host sudo systemctl daemon-reload
        host sudo systemctl enable "bootc-remote-play-tty2@${user}.service"
        echo "Enabled: bootc-remote-play-tty2@${user}.service"
        echo "Start it with: sudo systemctl start bootc-remote-play-tty2@${user}.service"
    '

disable-remote-play:
    @bash -euo pipefail -c '
        host() {
            if command -v flatpak-spawn >/dev/null 2>&1 && ([ -f /.containerenv ] || [ -f /run/.containerenv ]); then
                flatpak-spawn --host "$@"
            else
                "$@"
            fi
        }

        user="${SUDO_USER:-$USER}"
        echo "Disabling remote play for user: ${user}"
        host sudo systemctl disable --now "bootc-remote-play-tty2@${user}.service" || true
        host sudo rm -f /etc/bootc-remote-play/enabled
        echo "Disabled"
    '

remote-play-status:
    @bash -euo pipefail -c '
        host() {
            if command -v flatpak-spawn >/dev/null 2>&1 && ([ -f /.containerenv ] || [ -f /run/.containerenv ]); then
                flatpak-spawn --host "$@"
            else
                "$@"
            fi
        }

        user="${SUDO_USER:-$USER}"
        if host test -e /etc/bootc-remote-play/enabled; then
            echo "/etc/bootc-remote-play/enabled: present"
        else
            echo "/etc/bootc-remote-play/enabled: missing"
        fi
        host systemctl is-enabled "bootc-remote-play-tty2@${user}.service" || true
        host systemctl status --no-pager "bootc-remote-play-tty2@${user}.service" || true
    '

# Check for drift between system state and sources of truth (Containerfile, manifests)
check-drift:
    @/usr/bin/check-drift

# Check drift with JSON output
check-drift-json:
    @/usr/bin/check-drift --json
