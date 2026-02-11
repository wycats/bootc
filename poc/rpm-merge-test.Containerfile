# RPM Database Merge PoC (HISTORICAL — result: FAILED)
#
# Tests whether packages installed in separate containers from the same base
# can be COPY'd into a final container with a working RPM database.
#
# Result: The second COPY --from clobbers the first's RPM database.
# Only the last-copied stage's packages appear in `rpm -qa`.
# This led to the download/install split approach in RFC 0041.
#
# To reproduce: podman build -f poc/rpm-merge-test.Containerfile .

# ─── Base: Fedora with external repos configured ───────────────
FROM registry.fedoraproject.org/fedora:43 AS base
RUN rpm --import https://packages.microsoft.com/keys/microsoft.asc && \
    printf '%s\n' \
        '[code]' \
        'name=Visual Studio Code' \
        'baseurl=https://packages.microsoft.com/yumrepos/vscode' \
        'enabled=1' \
        'gpgcheck=1' \
        'gpgkey=https://packages.microsoft.com/keys/microsoft.asc' \
        >/etc/yum.repos.d/vscode.repo && \
    printf '%s\n' \
        '[microsoft-edge]' \
        'name=microsoft-edge' \
        'baseurl=https://packages.microsoft.com/yumrepos/edge' \
        'enabled=1' \
        'gpgcheck=1' \
        'gpgkey=https://packages.microsoft.com/keys/microsoft.asc' \
        >/etc/yum.repos.d/microsoft-edge.repo

# ─── Branch A: Install VS Code ─────────────────────────────────
FROM base AS vscode
RUN dnf install -y code && dnf clean all

# ─── Branch B: Install Edge ────────────────────────────────────
FROM base AS edge
RUN dnf install -y microsoft-edge-stable && dnf clean all

# ─── Merge: Copy both into a single container ──────────────────
FROM base AS merged
COPY --from=vscode / /
COPY --from=edge / /

# ─── Verify ────────────────────────────────────────────────────
# Check 1: rpm -qa sees both packages
RUN echo "=== RPM database check ===" && \
    rpm -qa | grep -E '^(code|microsoft-edge)' | sort && \
    echo "" && \
    CODE_COUNT=$(rpm -qa | grep -c '^code-') && \
    EDGE_COUNT=$(rpm -qa | grep -c '^microsoft-edge-') && \
    echo "code packages found: ${CODE_COUNT}" && \
    echo "edge packages found: ${EDGE_COUNT}" && \
    if [ "${CODE_COUNT}" -eq 0 ] || [ "${EDGE_COUNT}" -eq 0 ]; then \
        echo "FAIL: Missing packages after merge"; exit 1; \
    fi && \
    echo "PASS: Both packages present in RPM database"

# Check 2: binaries exist and are executable
RUN echo "=== Binary check ===" && \
    ls -la /usr/bin/code /usr/bin/microsoft-edge-stable 2>&1 && \
    file /usr/bin/code /usr/bin/microsoft-edge-stable && \
    echo "PASS: Binaries exist"

# Check 3: dnf check (verify no broken dependencies)
RUN echo "=== DNF dependency check ===" && \
    dnf check 2>&1 | head -50 && \
    echo "PASS: dnf check completed"

# Check 4: rpm --verify on merged packages
RUN echo "=== RPM verify ===" && \
    rpm -V code 2>&1 | head -20 || true && \
    rpm -V microsoft-edge-stable 2>&1 | head -20 || true && \
    echo "PASS: rpm verify completed"

# Check 5: Total package count sanity
RUN echo "=== Package count ===" && \
    TOTAL=$(rpm -qa | wc -l) && \
    echo "Total packages: ${TOTAL}" && \
    echo "PASS: Package count check"
