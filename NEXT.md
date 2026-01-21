# Next Phase: Phase 4+ — Polish & Quality

This document sketches potential Phase 4+ work. Phase 3 is tracked in [CURRENT.md](CURRENT.md).

---

## Immediate: Toolbox Image Update Workflow

The intended workflow is:

1. Get the toolbox behaving correctly **locally** (init hooks, exports, PATH).
2. Update `toolbox/Containerfile` to encode exactly what worked.
3. Open a PR and let CI rebuild/publish the image.
4. Once the PR builds the new image, validate that a distrobox upgrade works end-to-end.

### Current Status (2025-01-21)

**Local setup is complete and working:**

- Distrobox container `bootc-dev` created with the old image
- rustup and proto manually installed inside the container
- All exports working: cargo, rustc, rustfmt, clippy, rustup, node, pnpm, proto, nu
- Host PATH includes `~/.local/bin/distrobox`

**Containerfile is ready:**

The `toolbox/Containerfile` has been updated to bake in rustup and proto. Once CI rebuilds:

- Init hooks will work: `rustup default stable`, `rustup update stable`, `proto install node`
- No manual setup will be needed on fresh machines

**Remaining steps:**

1. [ ] Open PR with these changes
2. [ ] Wait for CI to rebuild `ghcr.io/wycats/bootc-toolbox:latest`
3. [ ] Test: `distrobox rm bootc-dev --force && bkt distrobox apply`
4. [ ] Verify all exports work without manual intervention

---

## Goals

Phase 4+ focuses on tightening the developer workflow and CI guarantees after Phase 3 closes the manifest→Containerfile loop.

---

## Candidates (Unscheduled)

### Manifest-driven toolbox Containerfile

The `toolbox/Containerfile` is currently manually maintained. It should eventually use managed sections like the main `Containerfile`:

- [ ] Create `manifests/toolbox-packages.json` for toolbox dev tools
- [ ] Extend `bkt containerfile` to support `--toolbox` flag
- [ ] Add managed sections: `TOOLBOX_PACKAGES`, `TOOLCHAIN_INSTALLERS`

See [docs/VISION.md](docs/VISION.md) for the philosophy.

### Changelog enforcement & visibility

- [ ] Add CI check: PR must have changelog entry
- [ ] Add CI check: No draft entries on merge
- [ ] Create MOTD integration for first-boot "What's New" (optional)

### Upstream polish

- [ ] Generate changelog entries for upstream updates
- [ ] Implement semver update policies

### Capture UX

- [ ] Add `--select` flag for interactive capture selection (future: TUI)

---

## Notes

- Changelog↔status integration was promoted to Phase 3 (see [CURRENT.md § 7](CURRENT.md#7-changelog-in-status)).
