FROM ghcr.io/ublue-os/bazzite:stable

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
    microsoft-edge-stable \
    papirus-icon-theme \
    rsms-inter-fonts \
    toolbox \
    curl \
    unzip \
    fontconfig \
    && dnf clean all

# starship (pinned to upstream version + verified by sha256)
COPY upstream/starship.version /tmp/starship.version
RUN set -eu; \
    tag="$(cat /tmp/starship.version)"; \
    asset="starship-x86_64-unknown-linux-gnu.tar.gz"; \
    curl -fsSL "https://github.com/starship/starship/releases/download/${tag}/${asset}" -o /tmp/${asset}; \
    curl -fsSL "https://github.com/starship/starship/releases/download/${tag}/${asset}.sha256" -o /tmp/${asset}.sha256; \
    expected="$(cat /tmp/${asset}.sha256)"; \
    echo "${expected}  /tmp/${asset}" | sha256sum -c -; \
    tar -xzf /tmp/${asset} -C /usr/local/bin starship; \
    chmod 0755 /usr/local/bin/starship; \
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
    tar -xzf /tmp/${asset} -C /usr/local/bin lazygit; \
    chmod 0755 /usr/local/bin/lazygit; \
    rm -f /tmp/${asset} /tmp/lazygit.checksums.txt /tmp/lazygit.version

# keyd (built from source at a pinned upstream tag)
COPY upstream/keyd.ref /tmp/keyd.ref
RUN set -eu; \
    ref="$(cat /tmp/keyd.ref)"; \
    dnf install -y git gcc make systemd-devel; \
    git clone --depth 1 --branch "${ref}" https://github.com/rvaiya/keyd.git /tmp/keyd; \
    make -C /tmp/keyd; \
    make -C /tmp/keyd install; \
    rm -rf /tmp/keyd /tmp/keyd.ref; \
    dnf remove -y git gcc make systemd-devel || true; \
    dnf clean all

# getnf (pinned to upstream ref + verified by sha256)
COPY upstream/getnf.ref /tmp/getnf.ref
COPY upstream/getnf.sha256 /tmp/getnf.sha256
RUN set -eu; \
    ref="$(cat /tmp/getnf.ref)"; \
    curl -fsSL "https://raw.githubusercontent.com/getnf/getnf/${ref}/getnf" -o /usr/local/bin/getnf; \
    echo "$(cat /tmp/getnf.sha256)  /usr/local/bin/getnf" | sha256sum -c -; \
    chmod 0755 /usr/local/bin/getnf; \
    rm -f /tmp/getnf.ref /tmp/getnf.sha256

# Fonts (system-wide): Inter (RPM) + JetBrainsMono Nerd Font (zip from nerd-fonts)
RUN set -eu; \
    mkdir -p /usr/share/fonts/nerd-fonts/JetBrainsMono; \
    curl -fsSL "https://github.com/ryanoasis/nerd-fonts/releases/latest/download/JetBrainsMono.zip" -o /tmp/JetBrainsMono.zip; \
    unzip -o /tmp/JetBrainsMono.zip -d /usr/share/fonts/nerd-fonts/JetBrainsMono; \
    rm -f /tmp/JetBrainsMono.zip; \
    fc-cache -f

# Host-level config extracted from wycats/asahi-env (now sourced here)
COPY system/keyd/default.conf /etc/keyd/default.conf

# First-login bootstrap (Flatpak + GNOME extensions)
RUN mkdir -p /usr/local/share/bootc-bootstrap
COPY manifests/flatpak-remotes.txt /usr/local/share/bootc-bootstrap/flatpak-remotes.txt
COPY manifests/flatpak-apps.txt /usr/local/share/bootc-bootstrap/flatpak-apps.txt
COPY manifests/gnome-extensions.txt /usr/local/share/bootc-bootstrap/gnome-extensions.txt
COPY manifests/gsettings.txt /usr/local/share/bootc-bootstrap/gsettings.txt
COPY scripts/bootc-bootstrap /usr/local/bin/bootc-bootstrap
COPY systemd/user/bootc-bootstrap.service /usr/lib/systemd/user/bootc-bootstrap.service
RUN chmod 0755 /usr/local/bin/bootc-bootstrap && \
    mkdir -p /usr/lib/systemd/user/default.target.wants && \
    ln -sf ../bootc-bootstrap.service /usr/lib/systemd/user/default.target.wants/bootc-bootstrap.service

# Custom ujust recipes (auto-imported as /usr/share/ublue-os/just/60-custom.just)
RUN mkdir -p /usr/share/ublue-os/just
COPY ujust/60-custom.just /usr/share/ublue-os/just/60-custom.just

# Optional host tweaks (off by default)
# NetworkManager: disable Wi-Fi power save (wifi.powersave=2)
ARG ENABLE_NM_DISABLE_WIFI_POWERSAVE=0
RUN mkdir -p /usr/local/share/bootc-optional/NetworkManager/conf.d
COPY system/NetworkManager/conf.d/default-wifi-powersave-on.conf /usr/local/share/bootc-optional/NetworkManager/conf.d/default-wifi-powersave-on.conf
RUN if [ "${ENABLE_NM_DISABLE_WIFI_POWERSAVE}" = "1" ]; then \
            install -Dpm0644 /usr/local/share/bootc-optional/NetworkManager/conf.d/default-wifi-powersave-on.conf /etc/NetworkManager/conf.d/default-wifi-powersave-on.conf; \
        fi

# NetworkManager: use iwd backend (wifi.backend=iwd) (Asahi-focused; off by default)
ARG ENABLE_NM_IWD_BACKEND=0
COPY system/NetworkManager/conf.d/wifi_backend.conf /usr/local/share/bootc-optional/NetworkManager/conf.d/wifi_backend.conf
RUN if [ "${ENABLE_NM_IWD_BACKEND}" = "1" ]; then \
            install -Dpm0644 /usr/local/share/bootc-optional/NetworkManager/conf.d/wifi_backend.conf /etc/NetworkManager/conf.d/wifi_backend.conf; \
        fi

# Asahi-only artifacts (off by default)
# NOTE: titdb.service.template contains a placeholder input device path; enable only after customizing it.
ARG ENABLE_ASAHI_TITDB=0
COPY system/asahi/titdb.service.template /usr/local/share/bootc-optional/asahi/titdb.service
RUN if [ "${ENABLE_ASAHI_TITDB}" = "1" ]; then \
            install -Dpm0644 /usr/local/share/bootc-optional/asahi/titdb.service /etc/systemd/system/titdb.service; \
            systemctl enable titdb.service || true; \
        fi

ARG ENABLE_ASAHI_BRCMFMAC=0
RUN mkdir -p /usr/local/share/bootc-optional/modprobe.d
COPY system/asahi/modprobe.d/brcmfmac.conf /usr/local/share/bootc-optional/modprobe.d/brcmfmac.conf
RUN if [ "${ENABLE_ASAHI_BRCMFMAC}" = "1" ]; then \
            install -Dpm0644 /usr/local/share/bootc-optional/modprobe.d/brcmfmac.conf /etc/modprobe.d/brcmfmac.conf; \
        fi

# systemd: journald log cap (off by default)
ARG ENABLE_JOURNALD_LOG_CAP=0
RUN mkdir -p /usr/local/share/bootc-optional/systemd/journald.conf.d
COPY system/systemd/journald.conf.d/10-journal-cap.conf /usr/local/share/bootc-optional/systemd/journald.conf.d/10-journal-cap.conf
RUN if [ "${ENABLE_JOURNALD_LOG_CAP}" = "1" ]; then \
            install -Dpm0644 /usr/local/share/bootc-optional/systemd/journald.conf.d/10-journal-cap.conf /etc/systemd/journald.conf.d/10-journal-cap.conf; \
        fi

# systemd: logind lid policy (off by default)
ARG ENABLE_LOGIND_LID_POLICY=0
RUN mkdir -p /usr/local/share/bootc-optional/systemd/logind.conf.d
COPY system/systemd/logind.conf.d/10-lid-policy.conf /usr/local/share/bootc-optional/systemd/logind.conf.d/10-lid-policy.conf
RUN if [ "${ENABLE_LOGIND_LID_POLICY}" = "1" ]; then \
            install -Dpm0644 /usr/local/share/bootc-optional/systemd/logind.conf.d/10-lid-policy.conf /etc/systemd/logind.conf.d/10-lid-policy.conf; \
        fi

# Dotfiles detected in system_profile.txt
COPY skel/.bashrc /etc/skel/.bashrc

# Nushell + toolbox-related defaults (profile addendum)
COPY skel/.config/nushell/config.nu /etc/skel/.config/nushell/config.nu
COPY skel/.config/nushell/env.nu /etc/skel/.config/nushell/env.nu
COPY skel/.config/containers/toolbox.conf /etc/skel/.config/containers/toolbox.conf
