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

# === COPR_REPOS (managed by bkt) ===
# No COPR repositories configured
# === END COPR_REPOS ===

# === KERNEL_ARGUMENTS (managed by bkt) ===
# No kernel arguments configured
# === END KERNEL_ARGUMENTS ===

# === SYSTEM_PACKAGES (managed by bkt) ===
RUN dnf install -y \
    1password \
    1password-cli \
    code \
    code-insiders \
    curl \
    fontconfig \
    gh \
    google-noto-sans-batak-fonts \
    google-noto-sans-inscriptional-pahlavi-fonts \
    google-noto-sans-inscriptional-parthian-fonts \
    google-noto-sans-meroitic-fonts \
    google-noto-sans-mongolian-fonts \
    jq \
    microsoft-edge-stable \
    papirus-icon-theme \
    rsms-inter-fonts \
    toolbox \
    unzip \
    virt-manager \
    xorg-x11-server-Xvfb \
    && dnf clean all
# === END SYSTEM_PACKAGES ===

# === SYSTEMD_UNITS (managed by bkt) ===
# No systemd units configured
# === END SYSTEMD_UNITS ===

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

# starship (pinned via upstream/manifest.json + verified by sha256)
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN set -eu; \
    tag="$(jq -r '.upstreams[] | select(.name == "starship") | .pinned.version' /tmp/upstream-manifest.json)"; \
    asset="starship-x86_64-unknown-linux-gnu.tar.gz"; \
    curl -fsSL "https://github.com/starship/starship/releases/download/${tag}/${asset}" -o /tmp/${asset}; \
    curl -fsSL "https://github.com/starship/starship/releases/download/${tag}/${asset}.sha256" -o /tmp/${asset}.sha256; \
    expected="$(cat /tmp/${asset}.sha256)"; \
    echo "${expected}  /tmp/${asset}" | sha256sum -c -; \
    tar -xzf /tmp/${asset} -C /usr/bin starship; \
    chmod 0755 /usr/bin/starship; \
    rm -f /tmp/${asset} /tmp/${asset}.sha256

# lazygit (pinned via upstream/manifest.json + verified by checksums.txt)
RUN set -eu; \
    tag="$(jq -r '.upstreams[] | select(.name == "lazygit") | .pinned.version' /tmp/upstream-manifest.json)"; \
    ver="${tag#v}"; \
    asset="lazygit_${ver}_linux_x86_64.tar.gz"; \
    curl -fsSL "https://github.com/jesseduffield/lazygit/releases/download/${tag}/${asset}" -o /tmp/${asset}; \
    curl -fsSL "https://github.com/jesseduffield/lazygit/releases/download/${tag}/checksums.txt" -o /tmp/lazygit.checksums.txt; \
    (cd /tmp && grep " ${asset}$" lazygit.checksums.txt | sha256sum -c -); \
    tar -xzf /tmp/${asset} -C /usr/bin lazygit; \
    chmod 0755 /usr/bin/lazygit; \
    rm -f /tmp/${asset} /tmp/lazygit.checksums.txt

# keyd (built from source at pinned tag from upstream/manifest.json)
# FORCE_SYSTEMD=1 is needed because /run/systemd/system doesn't exist in container builds
RUN set -eu; \
    ref="$(jq -r '.upstreams[] | select(.name == "keyd") | .pinned.version' /tmp/upstream-manifest.json)"; \
    dnf install -y git gcc make systemd-devel; \
    git clone --depth 1 --branch "${ref}" https://github.com/rvaiya/keyd.git /tmp/keyd; \
    make -C /tmp/keyd; \
    make -C /tmp/keyd PREFIX=/usr FORCE_SYSTEMD=1 install; \
    rm -rf /tmp/keyd; \
    dnf remove -y git gcc make systemd-devel || true; \
    dnf clean all

# getnf (pinned via upstream/manifest.json + verified by sha256)
RUN set -eu; \
    ref="$(jq -r '.upstreams[] | select(.name == "getnf") | .pinned.commit' /tmp/upstream-manifest.json)"; \
    expected_sha="$(jq -r '.upstreams[] | select(.name == "getnf") | .pinned.sha256' /tmp/upstream-manifest.json)"; \
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
    chmod 0755 /usr/bin/getnf

# Fonts (system-wide): Inter (RPM) + JetBrainsMono Nerd Font (zip from nerd-fonts)
RUN set -eu; \
    mkdir -p /usr/share/fonts/nerd-fonts/JetBrainsMono; \
    curl -fsSL "https://github.com/ryanoasis/nerd-fonts/releases/latest/download/JetBrainsMono.zip" -o /tmp/JetBrainsMono.zip; \
    unzip -o /tmp/JetBrainsMono.zip -d /usr/share/fonts/nerd-fonts/JetBrainsMono; \
    rm -f /tmp/JetBrainsMono.zip; \
    fc-cache -f

# Fix emoji rendering in VS Code / Electron / Chromium apps
#
# Background: Chromium's Skia renderer has issues with:
# 1. COLRv1 vector emoji fonts (google-noto-emoji-fonts) - rendering failures
# 2. Large CBDT bitmap fonts (Noto Color Emoji 10.6MB) - can cause issues
#
# Solution: Use Twemoji (3.2MB CBDT bitmap font) from twitter-twemoji-fonts RPM.
# Twemoji is smaller and more compatible with Chromium's renderer.
#
# We remove the COLRv1 packages but keep Twemoji as the primary emoji font.
# The fontconfig in 99-emoji-fix.conf sets Twemoji as the preferred emoji font.
RUN dnf remove -y google-noto-emoji-fonts google-noto-color-emoji-fonts || true; \
    fc-cache -f

COPY system/fontconfig/99-emoji-fix.conf /etc/fonts/conf.d/99-emoji-fix.conf

# Bibata cursor theme (pinned via upstream/manifest.json + verified by sha256)
RUN set -eu; \
    version="$(jq -r '.upstreams[] | select(.name == "bibata-cursor") | .pinned.version' /tmp/upstream-manifest.json)"; \
    expected_sha="$(jq -r '.upstreams[] | select(.name == "bibata-cursor") | .pinned.sha256' /tmp/upstream-manifest.json)"; \
    asset="$(jq -r '.upstreams[] | select(.name == "bibata-cursor") | .source.asset_pattern' /tmp/upstream-manifest.json)"; \
    curl -fsSL "https://github.com/ful1e5/Bibata_Cursor/releases/download/${version}/${asset}" -o /tmp/${asset}; \
    echo "${expected_sha}  /tmp/${asset}" | sha256sum -c -; \
    mkdir -p /usr/share/icons; \
    tar -xJf /tmp/${asset} -C /usr/share/icons; \
    rm -f /tmp/${asset}

# WhiteSur icon theme (pinned via upstream/manifest.json, installed system-wide)
# Note: Using commit SHA instead of tag for immutability. The install.sh script
# is simple (copies files only, no network calls). Full tree verification is
# deferred to a future upstream management system.
RUN set -eu; \
    ref="$(jq -r '.upstreams[] | select(.name == "whitesur-icons") | .pinned.commit' /tmp/upstream-manifest.json)"; \
    dnf install -y git; \
    git clone --filter=blob:none https://github.com/vinceliuice/WhiteSur-icon-theme.git /tmp/whitesur-icons; \
    cd /tmp/whitesur-icons && git checkout "${ref}" && ./install.sh -d /usr/share/icons; \
    rm -rf /tmp/whitesur-icons; \
    dnf remove -y git || true; \
    dnf clean all

# Clean up upstream manifest (no longer needed after this point)
RUN rm -f /tmp/upstream-manifest.json

# Host-level config extracted from wycats/asahi-env (now sourced here)
COPY system/keyd/default.conf /etc/keyd/default.conf
# Enable keyd service (create symlink manually since systemctl doesn't work in containers)
RUN mkdir -p /usr/lib/systemd/system/multi-user.target.wants && \
    ln -sf ../keyd.service /usr/lib/systemd/system/multi-user.target.wants/keyd.service

# Polkit rules for bkt admin (passwordless bootc/rpm-ostree for wheel group)
COPY system/polkit-1/rules.d/50-bkt-admin.rules /etc/polkit-1/rules.d/50-bkt-admin.rules

# First-login bootstrap (Flatpak + GNOME extensions + host shims)
RUN mkdir -p /usr/share/bootc-bootstrap /usr/share/bootc
COPY manifests/flatpak-remotes.json /usr/share/bootc-bootstrap/flatpak-remotes.json
COPY manifests/flatpak-apps.json /usr/share/bootc-bootstrap/flatpak-apps.json
COPY manifests/gnome-extensions.json /usr/share/bootc-bootstrap/gnome-extensions.json
COPY manifests/gsettings.json /usr/share/bootc-bootstrap/gsettings.json
COPY manifests/host-shims.json /usr/share/bootc-bootstrap/host-shims.json
# Repository identity (for bkt --pr workflow)
COPY repo.json /usr/share/bootc/repo.json
COPY scripts/bootc-bootstrap /usr/bin/bootc-bootstrap
COPY scripts/bootc-apply /usr/bin/bootc-apply
COPY scripts/bootc-repo /usr/bin/bootc-repo
# bkt CLI (pre-built by CI, placed in scripts/ during workflow)
COPY scripts/bkt /usr/bin/bkt
# systemd units: user-level bootstrap and system-level apply
COPY systemd/user/bootc-bootstrap.service /usr/lib/systemd/user/bootc-bootstrap.service
COPY systemd/user/bootc-capture.service /usr/lib/systemd/user/bootc-capture.service
COPY systemd/user/bootc-capture.timer /usr/lib/systemd/user/bootc-capture.timer
COPY systemd/system/bootc-apply.service /usr/lib/systemd/system/bootc-apply.service
RUN chmod 0755 /usr/bin/bootc-bootstrap /usr/bin/bootc-apply /usr/bin/bootc-repo /usr/bin/bkt && \
    mkdir -p /usr/lib/systemd/user/default.target.wants && \
    ln -sf ../bootc-bootstrap.service /usr/lib/systemd/user/default.target.wants/bootc-bootstrap.service && \
    mkdir -p /usr/lib/systemd/user/timers.target.wants && \
    ln -sf ../bootc-capture.timer /usr/lib/systemd/user/timers.target.wants/bootc-capture.timer && \
    mkdir -p /usr/lib/systemd/system/multi-user.target.wants && \
    ln -sf ../bootc-apply.service /usr/lib/systemd/system/multi-user.target.wants/bootc-apply.service

# Custom ujust recipes (auto-imported as /usr/share/ublue-os/just/60-custom.just)
RUN mkdir -p /usr/share/ublue-os/just
COPY ujust/60-custom.just /usr/share/ublue-os/just/60-custom.just

# Integrated topgrade configuration
COPY system/etc/topgrade.toml /etc/topgrade.toml

# VM tuning for reduced degradation over time
COPY system/etc/sysctl.d/99-bootc-vm-tuning.conf /etc/sysctl.d/99-bootc-vm-tuning.conf

# Persist kernel arguments for zswap performance tuning
RUN mkdir -p /usr/lib/bootc/kargs.d && \
    echo "zswap.enabled=1 zswap.compressor=lz4 zswap.zpool=zsmalloc zswap.max_pool_percent=25" \
    > /usr/lib/bootc/kargs.d/zswap.karg

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

# Nushell config
COPY skel/.config/nushell/config.nu /etc/skel/.config/nushell/config.nu
COPY skel/.config/nushell/env.nu /etc/skel/.config/nushell/env.nu

# === HOST_SHIMS (managed by bkt) ===
RUN set -eu; \
    mkdir -p /usr/etc/skel/.local/toolbox/shims /usr/etc/skel/.local/bin; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBib290YyAiJEAiCg==' | base64 -d > /usr/etc/skel/.local/toolbox/shims/bootc && \
    chmod 0755 /usr/etc/skel/.local/toolbox/shims/bootc && \
    ln -sf ../toolbox/shims/bootc /usr/etc/skel/.local/bin/bootc; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBmbGF0cGFrICIkQCIK' | base64 -d > /usr/etc/skel/.local/toolbox/shims/flatpak && \
    chmod 0755 /usr/etc/skel/.local/toolbox/shims/flatpak && \
    ln -sf ../toolbox/shims/flatpak /usr/etc/skel/.local/bin/flatpak; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBqb3VybmFsY3RsICIkQCIK' | base64 -d > /usr/etc/skel/.local/toolbox/shims/journalctl && \
    chmod 0755 /usr/etc/skel/.local/toolbox/shims/journalctl && \
    ln -sf ../toolbox/shims/journalctl /usr/etc/skel/.local/bin/journalctl; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBwb2RtYW4gIiRAIgo=' | base64 -d > /usr/etc/skel/.local/toolbox/shims/podman && \
    chmod 0755 /usr/etc/skel/.local/toolbox/shims/podman && \
    ln -sf ../toolbox/shims/podman /usr/etc/skel/.local/bin/podman; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBycG0tb3N0cmVlICIkQCIK' | base64 -d > /usr/etc/skel/.local/toolbox/shims/rpm-ostree && \
    chmod 0755 /usr/etc/skel/.local/toolbox/shims/rpm-ostree && \
    ln -sf ../toolbox/shims/rpm-ostree /usr/etc/skel/.local/bin/rpm-ostree; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBzeXN0ZW1jdGwgIiRAIgo=' | base64 -d > /usr/etc/skel/.local/toolbox/shims/systemctl && \
    chmod 0755 /usr/etc/skel/.local/toolbox/shims/systemctl && \
    ln -sf ../toolbox/shims/systemctl /usr/etc/skel/.local/bin/systemctl; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCB1anVzdCAiJEAiCg==' | base64 -d > /usr/etc/skel/.local/toolbox/shims/ujust && \
    chmod 0755 /usr/etc/skel/.local/toolbox/shims/ujust && \
    ln -sf ../toolbox/shims/ujust /usr/etc/skel/.local/bin/ujust
# === END HOST_SHIMS ===

# === RPM VERSION SNAPSHOT ===
# Capture installed versions of system packages for OCI label embedding.
# This file is read by the build workflow to create org.wycats.bootc.rpm.versions label.
RUN rpm -qa --qf '%{NAME}\t%{EVR}\n' | sort > /usr/share/bootc/rpm-versions.txt
# === END RPM VERSION SNAPSHOT ===
