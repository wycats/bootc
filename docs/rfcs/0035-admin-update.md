# RFC 0035: `bkt admin update`

- Feature Name: `bkt_admin_update`
- Start Date: 2026-02-02
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Add a unified `bkt admin update` command that drives the full capture-first update workflow: capture drift, commit and push (direct or PR), wait for CI, verify image readiness, then stage the new image with `bootc upgrade`, optionally rebooting. The command is safe to re-run, reports progress during long-running operations, and fails fast when required conditions are not met.

## Motivation

### The Problem

The capture-first workflow is effective but fragmented:

1. Users make local changes (native tools or `bkt`)
2. `bkt capture` updates manifests
3. Users commit and push changes
4. CI builds a new image
5. `bootc upgrade` stages it

Today this is manual and error-prone. There is no single command that reliably tells you “the image for my current state is ready and staged.” This gap makes updates slower, encourages partial workflows, and makes it harder to automate or teach a standard “ready to update” path.

### The Goal

Provide a single “do the right thing” command for admins that:

- Captures drift
- Pushes a commit that CI can build
- Waits for the image to be ready
- Stages it on the system
- Optionally reboots

This command complements RFC 0034 (usroverlay integration): after testing changes in an ephemeral overlay, `bkt admin update` is the canonical path to commit those changes into the real image.

## Guide-level Explanation

### Proposed CLI

```bash
bkt admin update [--reboot] [--skip-capture] [--dry-run] [--force] [--timeout <duration>] [--pr]
```

Flags:

- `--reboot`: Automatically reboot after successful upgrade
- `--skip-capture`: Skip capture phase (assume manifests are current)
- `--dry-run`: Show what would happen without executing
- `--force`: Proceed even if the working tree is dirty
- `--timeout <duration>`: Maximum time to wait for CI and image readiness (default: 30m)
- `--pr`: Use the standard `bkt` PR workflow instead of direct push

### High-Level Workflow

1. **Capture phase**
   - Run `bkt capture` unless `--skip-capture`
  - If capture finds changes, commit and push them (or open a PR when `--pr` is set)
  - If the working tree is dirty before capture, warn and require `--force` (unless `--dry-run`)

2. **Wait for CI phase**
  - Track the commit SHA that was pushed (or PR head SHA)
  - Poll GitHub Actions for the commit status
  - Stop immediately if the build fails

3. **Image readiness phase**
  - Poll the registry for the expected image digest
  - Confirm the image metadata matches the commit SHA

4. **Upgrade phase**
  - Run `bootc upgrade`
  - Wait for download completion
  - Report the staged image details (digest, timestamp, image ref)

5. **Optional reboot**
   - If `--reboot`, reboot after successful staging
   - Default behavior is to stage only

### Example Run

```bash
$ bkt admin update
✓ Capture: no drift detected
✓ Working tree clean
✓ Latest commit already on remote
⟳ Waiting for CI to finish for 4f6d2d1...
✓ CI succeeded
⟳ Waiting for image digest...
✓ Image ready: ghcr.io/ublue-os/bootc:stable@sha256:...
⟳ Running bootc upgrade...
✓ Staged image: ghcr.io/ublue-os/bootc:stable@sha256:...
ℹ Reboot to apply: systemctl reboot
```

### Example with Drift

```bash
$ bkt admin update
⟳ Running bkt capture...
✓ Drift detected: 3 files updated
⟳ Committing changes...
✓ Commit: 8a12c4c "capture: update manifests"
⟳ Pushing to origin...
✓ Pushed 8a12c4c
⟳ Waiting for CI...
✓ CI succeeded
⟳ Waiting for image digest...
✓ Image ready: ghcr.io/ublue-os/bootc:stable@sha256:...
⟳ Running bootc upgrade...
✓ Staged image: ghcr.io/ublue-os/bootc:stable@sha256:...
```

### Example Failure

```bash
$ bkt admin update
⟳ Waiting for CI...
✗ CI failed for 8a12c4c
  - See: https://github.com/.../actions/runs/123456
  - Aborting before bootc upgrade
```

## Challenges and Solutions

| Challenge | Proposed Solution |
| --- | --- |
| How to know CI is building our commit? | Track commit SHA, poll GitHub Actions API for runs on that SHA |
| How to know image is ready? | Poll registry for digest with metadata matching the commit SHA |
| What if CI fails? | Report failure, stop before upgrade |
| What if user has uncommitted work? | Warn and require `--force` or abort |
| How long to wait? | Configurable timeout, default 30 minutes |

## Reference-level Explanation

### Command Semantics

- **Idempotent**: If the repo is already up to date, CI already passed, and the staged image matches the latest build, the command performs no changes and exits cleanly.
- **Safe interruption**: Ctrl+C is safe at any point. The command must avoid leaving partial state beyond its own commit/push.
- **Progress reporting**: Long-running waits emit periodic updates (CI polling, registry check, image download).

### Phases and Behavior

#### Capture Phase

- When `--skip-capture` is not set:
  1. Check for a clean working tree.
    - If dirty, warn and require `--force` (unless `--dry-run`).
  2. Run `bkt capture`.
  3. If capture changes files:
     - Create a commit (e.g. `capture: update manifests`)
    - Push to the configured remote (or open a PR when `--pr` is set)
  4. If no changes, skip commit/push and keep current HEAD SHA.

#### PR Flow (`--pr`)

- Use the standard `bkt` PR workflow:
  - Create/update a PR branch with the capture commit.
  - Push to remote and open/refresh a PR.
  - Track the PR head SHA for CI polling.
- If the repo publishes PR images, `bkt admin update --pr` may proceed to upgrade using that image.
- If PR images are not available, stop after CI success and report the PR URL with next steps.

#### Wait for CI Phase

- Record the SHA that is expected to produce the image.
- Query GitHub Actions for workflow runs on that SHA.
- Wait until a workflow completes successfully, respecting `--timeout`.
- If the workflow fails, stop and present:
  - Failure status
  - Direct link to the run
  - The commit SHA that failed

#### Image Readiness Phase

- Query the container registry for the target image reference.
- Confirm the digest is new and that image metadata includes the commit SHA.
- Prefer `org.opencontainers.image.revision` as the source of truth.

#### Upgrade Phase

- Run `bootc upgrade`.
- Monitor the command output for download progress.
- After completion, read staged deployment information and report:
  - Image reference
  - Digest
  - Build timestamp (if available)

#### Optional Reboot

- If `--reboot` is set, reboot after a successful upgrade.
- Otherwise, provide the standard reboot hint.

### Offline and Network Failure Handling

- If network is unavailable during the CI wait phase:
  - Report the network error clearly
  - Offer a retry suggestion
  - Exit without calling `bootc upgrade`
- If registry checks fail during upgrade readiness:
  - Treat as a non-fatal error only if a staged image already matches the expected commit
  - Otherwise fail and report the missing image

### Data Flow and State

| Phase | Input | Output | Notes |
| --- | --- | --- | --- |
| Capture | working tree, system state | commit SHA | No-op if no drift |
| CI Wait | commit SHA | success + workflow URL | Failure stops flow |
| Image Ready | image ref + metadata | digest | Must match commit SHA |
| Upgrade | image ref + digest | staged deployment | `bootc upgrade` is authoritative |
| Reboot | staged deployment | system reboot | Optional |

## Rationale and Alternatives

### Why a new command instead of a script?

A native command can:

- Enforce idempotency and safe defaults
- Provide consistent progress reporting
- Integrate with existing error handling and status output
- Reduce operator error

### Alternatives Considered

- **External shell script**: Easy to write but hard to standardize and test. Error messages and edge-case behavior would diverge across users.
- **Enhancing `bkt capture` only**: Doesn’t solve the CI wait and upgrade gap.

## Drawbacks

1. **More GitHub coupling**: CI polling requires GitHub API access.
2. **Potential long waits**: Waiting for CI can be slow; users might prefer to do this manually.
3. **More complex error paths**: Failures can happen in multiple phases.

## Prior Art

- `bkt admin` workflows (RFC 0004)
- Dev/System command structure (RFC 0020)
- Capture-first workflow (RFC 0034 and docs/WORKFLOW.md)

## Unresolved Questions

1. **Workflow selection**: Which GitHub Actions workflows should be treated as authoritative for “image ready”? (single workflow name vs tag-based discovery)
2. **Multi-remote support**: How should the command behave if multiple remotes exist or if the default remote is not GitHub?
3. **Auth strategy**: Should we rely on `gh` CLI, a token in env, or both?
4. **PR image naming**: If `--pr` is used, what tag pattern should be considered authoritative?

## Future Possibilities

- `bkt admin update --watch` to stream CI logs while waiting
- `bkt admin update --since <sha>` to force a different target commit
- `bkt admin update --schedule` for cron-friendly staging

## Implementation Plan

1. Add `bkt admin update` subcommand with the proposed flags
2. Implement capture phase orchestration (reuse `bkt capture` logic)
3. Add GitHub Actions polling utilities (token/gh fallback)
4. Implement image readiness checks (registry digest lookup)
5. Integrate `bootc upgrade` execution and staged deployment reporting
6. Add structured progress output and failure messages
7. Document in README and workflow docs
