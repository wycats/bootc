# Next Phase: Phase 4+ — Polish & Quality

This document sketches potential Phase 4+ work. Phase 3 is tracked in [CURRENT.md](CURRENT.md).

---

## Goals

Phase 4+ focuses on tightening the developer workflow and CI guarantees after Phase 3 closes the manifest→Containerfile loop.

---

## Candidates (Unscheduled)

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
