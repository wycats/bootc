FROM ghcr.io/ublue-os/bazzite-gnome:stable

# Layered packages detected in system_profile.txt
RUN set -eu; \
    mkdir -p /var/opt /var/usrlocal/bin; \
    rpm --import https://packages.microsoft.com/keys/microsoft.asc; \
        printf '%s\n' \
            '[code]' \
            'name=Visual Studio Code' \
            'baseurl=https://packages.microsoft.com/yumrepos/vscode' \
            'enabled=1' \
            'gpgcheck=1' \
            'gpgkey=https://packages.microsoft.com/keys/microsoft.asc' \
            >/etc/yum.repos.d/vscode.repo; \
        printf '%s\n' \
            '[microsoft-edge]' \
            'name=microsoft-edge' \
            'baseurl=https://packages.microsoft.com/yumrepos/edge' \
            'enabled=1' \
            'gpgcheck=1' \
            'gpgkey=https://packages.microsoft.com/keys/microsoft.asc' \
            >/etc/yum.repos.d/microsoft-edge.repo; \
        rpm --import https://downloads.1password.com/linux/keys/1password.asc; \
        printf '%s\n' \
            '[1password]' \
            'name=1Password Stable Channel' \
            'baseurl=https://downloads.1password.com/linux/rpm/stable/$basearch' \
            'enabled=1' \
            'gpgcheck=1' \
            'gpgkey=https://downloads.1password.com/linux/keys/1password.asc' \
            >/etc/yum.repos.d/1password.repo

RUN dnf install -y \
    1password \
    1password-cli \
    code \
    gh \
    jq \
    microsoft-edge-stable \
    papirus-icon-theme \
    xorg-x11-server-Xvfb \
    rsms-inter-fonts \
    toolbox \
    curl \
    unzip \
    fontconfig \
    && dnf clean all

# Relocate /opt to /usr/lib/opt for ostree compatibility
# On ostree, /opt -> /var/opt which is persistent and NOT updated on upgrade.
# Moving to /usr/lib/opt makes it part of the immutable image.
RUN set -eu; \
    if [ -d /opt ] && [ "$(ls -A /opt 2>/dev/null)" ]; then \
        mkdir -p /usr/lib/opt; \
        cp -a /opt/. /usr/lib/opt/; \
        rm -rf /opt/*; \
    fi

# Create systemd tmpfiles rule to symlink /var/opt contents from /usr/lib/opt
RUN printf '%s\n' \
    '# Symlink /opt contents from immutable /usr/lib/opt' \
    'L+ /var/opt/1Password - - - - /usr/lib/opt/1Password' \
    'L+ /var/opt/microsoft - - - - /usr/lib/opt/microsoft' \
    >/usr/lib/tmpfiles.d/bootc-opt.conf

# starship (pinned to upstream version + verified by sha256)
COPY upstream/starship.version /tmp/starship.version
RUN set -eu; \
    tag="$(cat /tmp/starship.version)"; \
    asset="starship-x86_64-unknown-linux-gnu.tar.gz"; \
    curl -fsSL "https://github.com/starship/starship/releases/download/${tag}/${asset}" -o /tmp/${asset}; \
    curl -fsSL "https://github.com/starship/starship/releases/download/${tag}/${asset}.sha256" -o /tmp/${asset}.sha256; \
    expected="$(cat /tmp/${asset}.sha256)"; \
    echo "${expected}  /tmp/${asset}" | sha256sum -c -; \
    tar -xzf /tmp/${asset} -C /usr/bin starship; \
    chmod 0755 /usr/bin/starship; \
    rm -f /tmp/${asset} /tmp/${asset}.sha256 /tmp/starship.version

# lazygit (pinned to upstream version + verified by checksums.txt)
COPY upstream/lazygit.version /tmp/lazygit.version
RUN set -eu; \
    tag="$(cat /tmp/lazygit.version)"; \
    ver="${tag#v}"; \
    asset="lazygit_${ver}_linux_x86_64.tar.gz"; \
    curl -fsSL "https://github.com/jesseduffield/lazygit/releases/download/${tag}/${asset}" -o /tmp/${asset}; \
    curl -fsSL "https://github.com/jesseduffield/lazygit/releases/download/${tag}/checksums.txt" -o /tmp/lazygit.checksums.txt; \
    (cd /tmp && grep " ${asset}$" lazygit.checksums.txt | sha256sum -c -); \
    tar -xzf /tmp/${asset} -C /usr/bin lazygit; \
    chmod 0755 /usr/bin/lazygit; \
    rm -f /tmp/${asset} /tmp/lazygit.checksums.txt /tmp/lazygit.version

# keyd (built from source at a pinned upstream tag)
COPY upstream/keyd.ref /tmp/keyd.ref
RUN set -eu; \
    ref="$(cat /tmp/keyd.ref)"; \
    dnf install -y git gcc make systemd-devel; \
    git clone --depth 1 --branch "${ref}" https://github.com/rvaiya/keyd.git /tmp/keyd; \
    make -C /tmp/keyd; \
    make -C /tmp/keyd PREFIX=/usr install; \
    rm -rf /tmp/keyd /tmp/keyd.ref; \
    dnf remove -y git gcc make systemd-devel || true; \
    dnf clean all

# getnf (pinned to upstream ref + verified by sha256)
COPY upstream/getnf.ref /tmp/getnf.ref
COPY upstream/getnf.sha256 /tmp/getnf.sha256
RUN set -eu; \
    ref="$(cat /tmp/getnf.ref)"; \
    expected_sha="$(cat /tmp/getnf.sha256)"; \
    ok=0; \
    for attempt in 1 2 3; do \
        curl -fsSL \
            --retry 5 \
            --retry-delay 2 \
            "https://raw.githubusercontent.com/getnf/getnf/${ref}/getnf" \
            -o /usr/bin/getnf; \
        if echo "${expected_sha}  /usr/bin/getnf" | sha256sum -c -; then \
            ok=1; \
            break; \
        fi; \
        echo "getnf checksum mismatch; retrying (${attempt}/3)" >&2; \
        sleep $((attempt * 2)); \
    done; \
    test "${ok}" = 1; \
    chmod 0755 /usr/bin/getnf; \
    rm -f /tmp/getnf.ref /tmp/getnf.sha256

# Fonts (system-wide): Inter (RPM) + JetBrainsMono Nerd Font (zip from nerd-fonts)
RUN set -eu; \
    mkdir -p /usr/share/fonts/nerd-fonts/JetBrainsMono; \
    curl -fsSL "https://github.com/ryanoasis/nerd-fonts/releases/latest/download/JetBrainsMono.zip" -o /tmp/JetBrainsMono.zip; \
    unzip -o /tmp/JetBrainsMono.zip -d /usr/share/fonts/nerd-fonts/JetBrainsMono; \
    rm -f /tmp/JetBrainsMono.zip; \
    fc-cache -f

# Fix emoji rendering in VS Code / Electron apps
# The COLRv1 vector font is incompatible with Electron's Skia renderer.
# Remove it and install the legacy bitmap version instead.
RUN dnf remove -y google-noto-color-emoji-fonts || true; \
    mkdir -p /usr/share/fonts/noto-emoji; \
    curl -fsSL "https://github.com/googlefonts/noto-emoji/raw/main/fonts/NotoColorEmoji.ttf" \
        -o /usr/share/fonts/noto-emoji/NotoColorEmoji.ttf; \
    fc-cache -f

COPY system/fontconfig/99-emoji-fix.conf /etc/fonts/conf.d/99-emoji-fix.conf

# Host-level config extracted from wycats/asahi-env (now sourced here)
COPY system/keyd/default.conf /etc/keyd/default.conf
RUN systemctl enable keyd.service

# First-login bootstrap (Flatpak + GNOME extensions + host shims)
RUN mkdir -p /usr/share/bootc-bootstrap
COPY manifests/flatpak-remotes.json /usr/share/bootc-bootstrap/flatpak-remotes.json
COPY manifests/flatpak-apps.json /usr/share/bootc-bootstrap/flatpak-apps.json
COPY manifests/gnome-extensions.json /usr/share/bootc-bootstrap/gnome-extensions.json
COPY manifests/gsettings.json /usr/share/bootc-bootstrap/gsettings.json
COPY manifests/host-shims.json /usr/share/bootc-bootstrap/host-shims.json
COPY scripts/bootc-bootstrap /usr/bin/bootc-bootstrap
COPY scripts/check-drift /usr/bin/check-drift
COPY scripts/shim /usr/bin/shim
COPY scripts/bootc-repo /usr/bin/bootc-repo
COPY systemd/user/bootc-bootstrap.service /usr/lib/systemd/user/bootc-bootstrap.service
RUN chmod 0755 /usr/bin/bootc-bootstrap /usr/bin/check-drift /usr/bin/shim /usr/bin/bootc-repo && \
    mkdir -p /usr/lib/systemd/user/default.target.wants && \
    ln -sf ../bootc-bootstrap.service /usr/lib/systemd/user/default.target.wants/bootc-bootstrap.service

# Custom ujust recipes (auto-imported as /usr/share/ublue-os/just/60-custom.just)
RUN mkdir -p /usr/share/ublue-os/just
COPY ujust/60-custom.just /usr/share/ublue-os/just/60-custom.just

# Optional: remote play / console mode (off by default; enabled via `ujust enable-remote-play`)
RUN mkdir -p /usr/share/bootc-optional/remote-play/bin \
    /usr/share/bootc-optional/remote-play/systemd
COPY system/remote-play/bootc-remote-play-tty2 /usr/share/bootc-optional/remote-play/bin/bootc-remote-play-tty2
COPY system/remote-play/bootc-remote-play-tty2@.service /usr/share/bootc-optional/remote-play/systemd/bootc-remote-play-tty2@.service

# Optional host tweaks (off by default)
# NetworkManager: disable Wi-Fi power save (wifi.powersave=2)
ARG ENABLE_NM_DISABLE_WIFI_POWERSAVE=0
RUN mkdir -p /usr/share/bootc-optional/NetworkManager/conf.d
COPY system/NetworkManager/conf.d/default-wifi-powersave-on.conf /usr/share/bootc-optional/NetworkManager/conf.d/default-wifi-powersave-on.conf
RUN if [ "${ENABLE_NM_DISABLE_WIFI_POWERSAVE}" = "1" ]; then \
            install -Dpm0644 /usr/share/bootc-optional/NetworkManager/conf.d/default-wifi-powersave-on.conf /etc/NetworkManager/conf.d/default-wifi-powersave-on.conf; \
        fi

# NetworkManager: use iwd backend (wifi.backend=iwd) (Asahi-focused; off by default)
ARG ENABLE_NM_IWD_BACKEND=0
COPY system/NetworkManager/conf.d/wifi_backend.conf /usr/share/bootc-optional/NetworkManager/conf.d/wifi_backend.conf
RUN if [ "${ENABLE_NM_IWD_BACKEND}" = "1" ]; then \
            install -Dpm0644 /usr/share/bootc-optional/NetworkManager/conf.d/wifi_backend.conf /etc/NetworkManager/conf.d/wifi_backend.conf; \
        fi

# Asahi-only artifacts (off by default)
# NOTE: titdb.service.template contains a placeholder input device path; enable only after customizing it.
ARG ENABLE_ASAHI_TITDB=0
COPY system/asahi/titdb.service.template /usr/share/bootc-optional/asahi/titdb.service
RUN if [ "${ENABLE_ASAHI_TITDB}" = "1" ]; then \
            install -Dpm0644 /usr/share/bootc-optional/asahi/titdb.service /etc/systemd/system/titdb.service; \
            systemctl enable titdb.service || true; \
        fi

ARG ENABLE_ASAHI_BRCMFMAC=0
RUN mkdir -p /usr/share/bootc-optional/modprobe.d
COPY system/asahi/modprobe.d/brcmfmac.conf /usr/share/bootc-optional/modprobe.d/brcmfmac.conf
RUN if [ "${ENABLE_ASAHI_BRCMFMAC}" = "1" ]; then \
            install -Dpm0644 /usr/share/bootc-optional/modprobe.d/brcmfmac.conf /etc/modprobe.d/brcmfmac.conf; \
        fi

# systemd: journald log cap (off by default)
ARG ENABLE_JOURNALD_LOG_CAP=0
RUN mkdir -p /usr/share/bootc-optional/systemd/journald.conf.d
COPY system/systemd/journald.conf.d/10-journal-cap.conf /usr/share/bootc-optional/systemd/journald.conf.d/10-journal-cap.conf
RUN if [ "${ENABLE_JOURNALD_LOG_CAP}" = "1" ]; then \
            install -Dpm0644 /usr/share/bootc-optional/systemd/journald.conf.d/10-journal-cap.conf /etc/systemd/journald.conf.d/10-journal-cap.conf; \
        fi

# systemd: logind lid policy (off by default)
ARG ENABLE_LOGIND_LID_POLICY=0
RUN mkdir -p /usr/share/bootc-optional/systemd/logind.conf.d
COPY system/systemd/logind.conf.d/10-lid-policy.conf /usr/share/bootc-optional/systemd/logind.conf.d/10-lid-policy.conf
RUN if [ "${ENABLE_LOGIND_LID_POLICY}" = "1" ]; then \
            install -Dpm0644 /usr/share/bootc-optional/systemd/logind.conf.d/10-lid-policy.conf /etc/systemd/logind.conf.d/10-lid-policy.conf; \
        fi

# Dotfiles detected in system_profile.txt
COPY skel/.bashrc /etc/skel/.bashrc

# Nushell + toolbox-related defaults (profile addendum)
COPY skel/.config/nushell/config.nu /etc/skel/.config/nushell/config.nu
COPY skel/.config/nushell/env.nu /etc/skel/.config/nushell/env.nu
COPY skel/.config/containers/toolbox.conf /etc/skel/.config/containers/toolbox.conf
