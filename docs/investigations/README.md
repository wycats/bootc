# Investigations

Records of empirical findings from debugging sessions where the root cause
is not fully understood. Each investigation documents what was observed,
what was measured, and what was changed — without overstating conclusions.

## Purpose

An investigation record is appropriate when:

- We have strong empirical data (strace output, exact error messages, measured counts)
- We made a change that fixed the symptom
- We do NOT have a complete root cause analysis that would support an RFC
- We expect the issue (or a variant of it) to recur

An investigation record is NOT:

- Scratch notes from an active debugging session
- A design proposal (that's an RFC)
- A changelog entry (that's a commit message)

## Format

Each investigation is a markdown file named `YYYY-MM-DD-short-description.md`.
It should include:

- **Symptom**: What failed and how
- **Environment**: OS, versions, filesystem, relevant configuration
- **Observations**: Exact error messages, measurements, strace output, etc.
- **What we changed**: The fix or workaround, with commit/PR references
- **What we don't know**: Explicitly stated gaps in understanding
- **References**: Links to upstream issues, PRs, documentation that informed the investigation

## Index

| Date | Investigation | Status |
|------|--------------|--------|
| 2026-02-26 | [btrfs xattrs hardlink limit during bootc upgrade](2026-02-26-btrfs-xattrs-hardlink-limit.md) | Fixed (workaround), RCA incomplete |
