# ── Tools stage ──────────────────────────────────────────────────────────────
# Static musl binary for build-time operations (built by CI)
FROM scratch AS tools
COPY scripts/bkt-build /bkt-build

# ── Base stage (repos configured) ────────────────────────────────────────────
FROM ghcr.io/ublue-os/bazzite-gnome:stable AS base
COPY --from=tools /bkt-build /usr/bin/bkt-build
COPY manifests/external-repos.json /tmp/external-repos.json
RUN set -eu; \
    mkdir -p /var/opt /var/usrlocal/bin; \
    bkt-build setup-repos

# ── RPM download stages (parallel, each downloads from one external repo) ────

FROM base AS dl-code
ARG CACHE_EPOCH_CODE=0
RUN bkt-build download-rpms code

FROM base AS dl-microsoft-edge
ARG CACHE_EPOCH_MICROSOFT_EDGE=0
RUN bkt-build download-rpms microsoft-edge

FROM base AS dl-1password
ARG CACHE_EPOCH_1PASSWORD=0
RUN bkt-build download-rpms 1password

# ── RPM install stages (per-package extraction with /opt relocation) ─────────

FROM base AS install-code
COPY --from=dl-code /rpms/ /tmp/rpms/
RUN rpm -i --nodb --noscripts --nodeps /tmp/rpms/*.rpm && rm -rf /tmp/rpms

FROM base AS install-microsoft-edge
COPY --from=dl-microsoft-edge /rpms/ /tmp/rpms/
RUN set -eu; \
    rpm -i --nodb --noscripts --nodeps /tmp/rpms/*.rpm; \
    mkdir -p /usr/lib/opt; \
    if [ -d /opt/microsoft ]; then cp -a /opt/microsoft/. /usr/lib/opt/microsoft/; rm -rf /opt/microsoft; fi; \
    rm -rf /tmp/rpms

FROM base AS install-1password
COPY --from=dl-1password /rpms/ /tmp/rpms/
RUN set -eu; \
    rpm -i --nodb --noscripts --nodeps /tmp/rpms/*.rpm; \
    mkdir -p /usr/lib/opt; \
    if [ -d /opt/1Password ]; then cp -a /opt/1Password/. /usr/lib/opt/1Password/; rm -rf /opt/1Password; fi; \
    rm -rf /tmp/rpms

# ── Bundled packages merged stage (reduces deployment layer count) ───────────

FROM scratch AS install-bundled
COPY --from=install-1password / /

# ── Upstream fetch stages (parallel, each installs one upstream entry) ───────

FROM base AS fetch-starship
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN bkt-build fetch starship

FROM base AS fetch-lazygit
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN bkt-build fetch lazygit

FROM base AS fetch-getnf
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN bkt-build fetch getnf

FROM base AS fetch-bibata-cursor
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN bkt-build fetch bibata-cursor

FROM base AS fetch-jetbrains-mono-nerd-font
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN bkt-build fetch jetbrains-mono-nerd-font

# ── Bespoke build stages (parallel, for script-type installs) ────────────────

FROM base AS build-keyd
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN <<'EOF'
# keyd: build from source at pinned tag
# FORCE_SYSTEMD=1 is needed because /run/systemd/system doesn't exist in container builds
set -eu
ref="$(jq -r '.upstreams[] | select(.name == "keyd") | .pinned.version' /tmp/upstream-manifest.json)"
dnf install -y git gcc make systemd-devel
git clone --depth 1 --branch "${ref}" https://github.com/rvaiya/keyd.git /tmp/keyd
make -C /tmp/keyd
make -C /tmp/keyd PREFIX=/usr FORCE_SYSTEMD=1 install
rm -rf /tmp/keyd
EOF

FROM base AS fetch-whitesur
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN <<'EOF'
# WhiteSur icon theme: clone at pinned commit and run vendor install script
# Using commit SHA instead of tag for immutability. The install.sh script
# is simple (copies files only, no network calls).
set -eu
ref="$(jq -r '.upstreams[] | select(.name == "whitesur-icons") | .pinned.commit' /tmp/upstream-manifest.json)"
dnf install -y git
git clone --filter=blob:none https://github.com/vinceliuice/WhiteSur-icon-theme.git /tmp/whitesur-icons
cd /tmp/whitesur-icons && git checkout "${ref}" && ./install.sh -d /usr/share/icons
rm -rf /tmp/whitesur-icons
EOF

# ── Wrapper build stage (parallel, from manifest) ────────────────────────────

FROM rust:slim AS build-wrappers
RUN mkdir -p /out
COPY wrappers/vscode-wrapper/src/main.rs /tmp/vscode-wrapper.rs
RUN rustc --edition 2021 -O -o /out/vscode-wrapper /tmp/vscode-wrapper.rs
COPY wrappers/msedge-wrapper/src/main.rs /tmp/msedge-wrapper.rs
RUN rustc --edition 2021 -O -o /out/msedge-wrapper /tmp/msedge-wrapper.rs

# ── Config collector (parallel, FROM scratch) ────────────────────────────────
FROM scratch AS collect-config

# Emoji rendering fix config
COPY system/fontconfig/99-emoji-fix.conf /etc/fonts/conf.d/99-emoji-fix.conf

# keyd keyboard remapping config
COPY system/keyd/default.conf /etc/keyd/default.conf

# Polkit rules for bkt admin (passwordless bootc/rpm-ostree/flatpak for wheel group)
COPY system/polkit-1/rules.d/50-bkt-admin.rules /etc/polkit-1/rules.d/50-bkt-admin.rules

# First-login bootstrap (Flatpak + GNOME extensions + host shims)
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
COPY systemd/user/bkt-daemon.service /usr/lib/systemd/user/bkt-daemon.service
COPY systemd/system/bootc-apply.service /usr/lib/systemd/system/bootc-apply.service
# dbus-broker: raise soft fd limit to prevent session crashes under heavy container load
COPY systemd/user/dbus-broker.service.d/override.conf /usr/lib/systemd/user/dbus-broker.service.d/override.conf

# Custom ujust recipes (auto-imported as /usr/share/ublue-os/just/60-custom.just)
COPY ujust/60-custom.just /usr/share/ublue-os/just/60-custom.just

# Distrobox configuration (managed by bkt)
COPY distrobox.ini /etc/distrobox/distrobox.ini

# Integrated topgrade configuration
COPY system/etc/topgrade.toml /etc/topgrade.toml

# VM tuning for reduced degradation over time
COPY system/etc/sysctl.d/99-bootc-vm-tuning.conf /etc/sysctl.d/99-bootc-vm-tuning.conf

# Managed Edge policies to limit process bloat
COPY system/etc/opt/edge/policies/managed/performance.json /etc/opt/edge/policies/managed/performance.json

# Optional: remote play / console mode (off by default; enabled via `ujust enable-remote-play`)
COPY system/remote-play/bootc-remote-play-tty2 /usr/share/bootc-optional/remote-play/bin/bootc-remote-play-tty2
COPY system/remote-play/bootc-remote-play-tty2@.service /usr/share/bootc-optional/remote-play/systemd/bootc-remote-play-tty2@.service

# NetworkManager: disable Wi-Fi power save (wifi.powersave=2)
COPY system/NetworkManager/conf.d/default-wifi-powersave-on.conf /usr/share/bootc-optional/NetworkManager/conf.d/default-wifi-powersave-on.conf

# NetworkManager: use iwd backend (wifi.backend=iwd) (Asahi-focused; off by default)
COPY system/NetworkManager/conf.d/wifi_backend.conf /usr/share/bootc-optional/NetworkManager/conf.d/wifi_backend.conf

# Asahi-only artifacts (off by default)
# NOTE: titdb.service.template contains a placeholder input device path; enable only after customizing it.
COPY system/asahi/titdb.service.template /usr/share/bootc-optional/asahi/titdb.service
COPY system/asahi/modprobe.d/brcmfmac.conf /usr/share/bootc-optional/modprobe.d/brcmfmac.conf

# systemd: journald log cap (off by default)
COPY system/systemd/journald.conf.d/10-journal-cap.conf /usr/share/bootc-optional/systemd/journald.conf.d/10-journal-cap.conf

# systemd: logind lid policy (off by default)
COPY system/systemd/logind.conf.d/10-lid-policy.conf /usr/share/bootc-optional/systemd/logind.conf.d/10-lid-policy.conf

# Memory-managed app slices (RFC 0046): VS Code and Edge memory budgets
COPY systemd/user/app-vscode.slice /usr/lib/systemd/user/app-vscode.slice
COPY systemd/user/app-msedge.slice /usr/lib/systemd/user/app-msedge.slice

# systemd-oomd tuning: tighter pressure duration, per-app overrides
COPY system/etc/systemd/oomd.conf.d/10-bootc-tuning.conf /etc/systemd/oomd.conf.d/10-bootc-tuning.conf

# Nushell config
COPY skel/.config/nushell/config.nu /etc/skel/.config/nushell/config.nu
COPY skel/.config/nushell/env.nu /etc/skel/.config/nushell/env.nu

# ── Final image assembly ─────────────────────────────────────────────────────
FROM base AS image

# === COPR_REPOS (managed by bkt) ===
# No COPR repositories configured
# === END COPR_REPOS ===

# === KERNEL_ARGUMENTS (managed by bkt) ===
# No kernel arguments configured
# === END KERNEL_ARGUMENTS ===

# Import installed files from install-* stages (COPY --link for layer independence)
COPY --link --from=install-code / /
COPY --link --from=install-microsoft-edge / /
COPY --link --from=install-bundled / /

# === SYSTEM_PACKAGES (managed by bkt) ===
RUN dnf install -y \
    curl \
    distrobox \
    fontconfig \
    gh \
    google-noto-sans-batak-fonts \
    google-noto-sans-inscriptional-pahlavi-fonts \
    google-noto-sans-inscriptional-parthian-fonts \
    google-noto-sans-meroitic-fonts \
    google-noto-sans-mongolian-fonts \
    jq \
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

# Create systemd tmpfiles rule to symlink /var/opt contents from /usr/lib/opt
RUN printf '%s\n' \
    '# Symlink /opt contents from immutable /usr/lib/opt' \
    'L+ /var/opt/microsoft - - - - /usr/lib/opt/microsoft' \
    'L+ /var/opt/1Password - - - - /usr/lib/opt/1Password' \
    >/usr/lib/tmpfiles.d/bootc-opt.conf

# Finalize RPM database for external packages
COPY --from=dl-code /rpms/ /tmp/rpms-code/
COPY --from=dl-microsoft-edge /rpms/ /tmp/rpms-microsoft-edge/
COPY --from=dl-1password /rpms/ /tmp/rpms-1password/
RUN set -eu; \
    rpm -i --justdb --nodeps /tmp/rpms-code/*.rpm; \
    rpm -i --justdb --nodeps /tmp/rpms-microsoft-edge/*.rpm; \
    rpm -i --justdb --nodeps /tmp/rpms-1password/*.rpm; \
    ldconfig; \
    rm -rf /tmp/rpms-code /tmp/rpms-microsoft-edge /tmp/rpms-1password

# Clean up build-time artifacts (no longer needed after package install)
RUN rm -rf /tmp/external-repos.json /usr/bin/bkt-build

# ── COPY upstream outputs into final image ───────────────────────────────────

COPY --link --from=fetch-starship /usr/bin/starship /usr/bin/starship
COPY --link --from=fetch-lazygit /usr/bin/lazygit /usr/bin/lazygit
COPY --link --from=fetch-getnf /usr/bin/getnf /usr/bin/getnf
COPY --link --from=fetch-bibata-cursor /usr/share/icons/Bibata-Modern-Classic/ /usr/share/icons/Bibata-Modern-Classic/
COPY --link --from=fetch-jetbrains-mono-nerd-font /usr/share/fonts/nerd-fonts/JetBrainsMono/ /usr/share/fonts/nerd-fonts/JetBrainsMono/

# keyd outputs (built from source)
COPY --link --from=build-keyd /usr/bin/keyd /usr/bin/keyd
COPY --link --from=build-keyd /usr/bin/keyd-application-mapper /usr/bin/keyd-application-mapper
COPY --link --from=build-keyd /usr/lib/systemd/system/keyd.service /usr/lib/systemd/system/keyd.service
COPY --link --from=build-keyd /usr/share/keyd/ /usr/share/keyd/
COPY --link --from=build-keyd /usr/share/man/man1/keyd.1.gz /usr/share/man/man1/keyd.1.gz
COPY --link --from=build-keyd /usr/share/man/man1/keyd-application-mapper.1.gz /usr/share/man/man1/keyd-application-mapper.1.gz
COPY --link --from=build-keyd /usr/share/doc/keyd/ /usr/share/doc/keyd/

# WhiteSur icon theme
COPY --link --from=fetch-whitesur /usr/share/icons/WhiteSur/ /usr/share/icons/WhiteSur/
COPY --link --from=fetch-whitesur /usr/share/icons/WhiteSur-dark/ /usr/share/icons/WhiteSur-dark/

# ── Configuration overlay (from collect-config) ──────────────────────────────
COPY --link --from=collect-config / /

# Memory-managed application wrappers (from build-wrappers stage)
COPY --link --from=build-wrappers /out/vscode-wrapper /usr/bin/code
COPY --link --from=build-wrappers /out/msedge-wrapper /usr/bin/microsoft-edge-stable


# Optional host tweaks (off by default)

# NetworkManager: disable Wi-Fi power save (wifi.powersave=2)
ARG ENABLE_NM_DISABLE_WIFI_POWERSAVE=0

# NetworkManager: use iwd backend (wifi.backend=iwd) (Asahi-focused; off by default)
ARG ENABLE_NM_IWD_BACKEND=0

# Asahi-only artifacts (off by default)
# NOTE: titdb.service.template contains a placeholder input device path; enable only after customizing it.
ARG ENABLE_ASAHI_TITDB=0
ARG ENABLE_ASAHI_BRCMFMAC=0

# systemd: journald log cap (off by default)
ARG ENABLE_JOURNALD_LOG_CAP=0

# systemd: logind lid policy (off by default)
ARG ENABLE_LOGIND_LID_POLICY=0
# Post-overlay setup: chmod, symlinks, mkdir, kargs, staging dirs
RUN set -eu; \
    mkdir -p /usr/lib/systemd/system/multi-user.target.wants; \
    ln -sf ../keyd.service /usr/lib/systemd/system/multi-user.target.wants/keyd.service; \
    mkdir -p /usr/share/bootc-bootstrap /usr/share/bootc; \
    chmod 0755 /usr/bin/bootc-bootstrap /usr/bin/bootc-apply /usr/bin/bootc-repo /usr/bin/bkt; \
    mkdir -p /usr/lib/systemd/user/default.target.wants; \
    ln -sf ../bootc-bootstrap.service /usr/lib/systemd/user/default.target.wants/bootc-bootstrap.service; \
    ln -sf ../bkt-daemon.service /usr/lib/systemd/user/default.target.wants/bkt-daemon.service; \
    mkdir -p /usr/lib/systemd/user/timers.target.wants; \
    ln -sf ../bootc-capture.timer /usr/lib/systemd/user/timers.target.wants/bootc-capture.timer; \
    mkdir -p /usr/lib/systemd/system/multi-user.target.wants; \
    ln -sf ../bootc-apply.service /usr/lib/systemd/system/multi-user.target.wants/bootc-apply.service; \
    mkdir -p /usr/share/ublue-os/just; \
    mkdir -p /etc/opt/edge/policies/managed; \
    mkdir -p /usr/lib/bootc/kargs.d; \
    echo "zswap.enabled=1 zswap.compressor=lz4 zswap.zpool=zsmalloc zswap.max_pool_percent=25 transparent_hugepage=madvise" \
    > /usr/lib/bootc/kargs.d/tuning.karg; \
    mkdir -p /usr/share/bootc-optional/remote-play/bin /usr/share/bootc-optional/remote-play/systemd; \
    mkdir -p /usr/share/bootc-optional/NetworkManager/conf.d; \
    mkdir -p /usr/share/bootc-optional/modprobe.d; \
    mkdir -p /usr/share/bootc-optional/systemd/journald.conf.d; \
    mkdir -p /usr/share/bootc-optional/systemd/logind.conf.d; \
    mkdir -p /usr/etc/skel/.local/toolbox/shims /usr/etc/skel/.local/bin; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBib290YyAiJEAiCg==' | base64 -d \
    > /usr/etc/skel/.local/toolbox/shims/bootc && chmod 0755 /usr/etc/skel/.local/toolbox/shims/bootc && ln -sf ../toolbox/shims/bootc /usr/etc/skel/.local/bin/bootc; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBwb2RtYW4gIiRAIgo=' | base64 -d \
    > /usr/etc/skel/.local/toolbox/shims/docker && chmod 0755 /usr/etc/skel/.local/toolbox/shims/docker && ln -sf ../toolbox/shims/docker /usr/etc/skel/.local/bin/docker; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBmbGF0cGFrICIkQCIK' | base64 -d \
    > /usr/etc/skel/.local/toolbox/shims/flatpak && chmod 0755 /usr/etc/skel/.local/toolbox/shims/flatpak && ln -sf ../toolbox/shims/flatpak /usr/etc/skel/.local/bin/flatpak; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBqb3VybmFsY3RsICIkQCIK' | base64 -d \
    > /usr/etc/skel/.local/toolbox/shims/journalctl && chmod 0755 /usr/etc/skel/.local/toolbox/shims/journalctl && ln -sf ../toolbox/shims/journalctl /usr/etc/skel/.local/bin/journalctl; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBwb2RtYW4gIiRAIgo=' | base64 -d \
    > /usr/etc/skel/.local/toolbox/shims/podman && chmod 0755 /usr/etc/skel/.local/toolbox/shims/podman && ln -sf ../toolbox/shims/podman /usr/etc/skel/.local/bin/podman; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBycG0tb3N0cmVlICIkQCIK' | base64 -d \
    > /usr/etc/skel/.local/toolbox/shims/rpm-ostree && chmod 0755 /usr/etc/skel/.local/toolbox/shims/rpm-ostree && ln -sf ../toolbox/shims/rpm-ostree /usr/etc/skel/.local/bin/rpm-ostree; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCBzeXN0ZW1jdGwgIiRAIgo=' | base64 -d \
    > /usr/etc/skel/.local/toolbox/shims/systemctl && chmod 0755 /usr/etc/skel/.local/toolbox/shims/systemctl && ln -sf ../toolbox/shims/systemctl /usr/etc/skel/.local/bin/systemctl; \
    echo 'IyEvYmluL2Jhc2gKZXhlYyBmbGF0cGFrLXNwYXduIC0taG9zdCB1anVzdCAiJEAiCg==' | base64 -d \
    > /usr/etc/skel/.local/toolbox/shims/ujust && chmod 0755 /usr/etc/skel/.local/toolbox/shims/ujust && ln -sf ../toolbox/shims/ujust /usr/etc/skel/.local/bin/ujust; \
    if [ "${ENABLE_NM_DISABLE_WIFI_POWERSAVE}" = "1" ]; then install -Dpm0644 /usr/share/bootc-optional/NetworkManager/conf.d/default-wifi-powersave-on.conf /etc/NetworkManager/conf.d/default-wifi-powersave-on.conf; fi; \
    if [ "${ENABLE_NM_IWD_BACKEND}" = "1" ]; then install -Dpm0644 /usr/share/bootc-optional/NetworkManager/conf.d/wifi_backend.conf /etc/NetworkManager/conf.d/wifi_backend.conf; fi; \
    if [ "${ENABLE_ASAHI_TITDB}" = "1" ]; then install -Dpm0644 /usr/share/bootc-optional/asahi/titdb.service /etc/systemd/system/titdb.service; systemctl enable titdb.service || true; fi; \
    if [ "${ENABLE_ASAHI_BRCMFMAC}" = "1" ]; then install -Dpm0644 /usr/share/bootc-optional/modprobe.d/brcmfmac.conf /etc/modprobe.d/brcmfmac.conf; fi; \
    if [ "${ENABLE_JOURNALD_LOG_CAP}" = "1" ]; then install -Dpm0644 /usr/share/bootc-optional/systemd/journald.conf.d/10-journal-cap.conf /etc/systemd/journald.conf.d/10-journal-cap.conf; fi; \
    if [ "${ENABLE_LOGIND_LID_POLICY}" = "1" ]; then install -Dpm0644 /usr/share/bootc-optional/systemd/logind.conf.d/10-lid-policy.conf /etc/systemd/logind.conf.d/10-lid-policy.conf; fi

# Rebuild font cache after all font/icon COPYs and config overlay
RUN fc-cache -f

# === RPM VERSION SNAPSHOT ===
# Capture installed versions of system packages for OCI label embedding.
# This file is read by the build workflow to create org.wycats.bootc.rpm.versions label.
RUN rpm -qa --qf '%{NAME}\t%{EVR}\n' | sort > /usr/share/bootc/rpm-versions.txt
# === END RPM VERSION SNAPSHOT ===
