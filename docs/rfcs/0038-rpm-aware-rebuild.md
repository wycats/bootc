# RFC 0038: RPM-Aware Hourly Rebuild

## Problem

The current build pipeline checks for upstream bazzite base image changes hourly, but only catches RPM package updates via a forced nightly rebuild at 03:00 UTC. Third-party packages like Microsoft Edge (~weekly point releases), VS Code, and 1Password update frequently. This creates a gap of up to 24 hours where the running system has stale packages, causing:

1. **1Password nag**: 1Password detects the stale Edge version and constantly prompts the user to update — but the immutable OS can't self-update RPMs.
2. **Security lag**: Security patches in point releases aren't picked up until the next nightly build.
3. **User friction**: The user must manually trigger `bkt build trigger` or wait for the nightly to get fresh packages.

## Proposal

### 1. RPM Version Check in Hourly Cron

Add an RPM freshness check to the existing `check-upstream` job. During each hourly cron run, after the base image digest check, compare installed RPM versions against what's available in the repos.

#### Use Existing system-packages.json

All system packages defined in `manifests/system-packages.json` are checked. No separate "watched" list is needed — rebuilds are cheap (~15min of free CI time), and checking all packages ensures security updates are never missed.

#### Version Tracking via rpm-versions.txt

During each build, the Containerfile generates a version snapshot:

```dockerfile
RUN rpm -qa --qf '%{NAME}\t%{EVR}\n' | sort > /usr/share/bootc/rpm-versions.txt
```

The hourly check then:

1. Pulls the current published image
2. Extracts `/usr/share/bootc/rpm-versions.txt` via `docker run`
3. Queries the RPM repos for current versions of system packages
4. Compares — if any package has a newer version available, triggers a rebuild

Note: An OCI label approach was considered but rejected due to disk space constraints on GitHub runners — loading the full image to extract versions exceeds available space.

#### Container-Based Repo Query

The build runs on Ubuntu, which doesn't have the third-party RPM repos configured. Rather than duplicating repo setup on the runner, run the version check inside a container based on the current published image (which already has all repos configured):

```bash
docker run --rm ghcr.io/$REPO:latest \
  dnf repoquery --latest-limit=1 --qf '%{name}:%{version}-%{release}' \
  $(jq -r '.packages[].name' manifests/system-packages.json) \
  2>/dev/null | sort
```

Compare this output against the versions embedded in the OCI label.

### 2. Replace Nightly Forced Rebuild with RPM-Aware Check

The RPM-aware check replaces the current "blind" nightly forced rebuild. Instead of rebuilding every night at 03:00 regardless of changes, the hourly cron now checks for both:
- Upstream base image digest changes (existing behavior)
- RPM version changes in system packages (new behavior)

If neither has changed, no rebuild is triggered. This eliminates unnecessary builds while ensuring updates are caught within an hour of release.

### 3. Build Failure Notifications (Future)

If we're relying on hourly builds to keep the system fresh, we need to know when they break. This is deferred to a future RFC — the core value is in steps 1-2.

## Workflow Changes

### Modified `check-upstream` Job

```yaml
check-upstream:
  runs-on: ubuntu-latest
  outputs:
    upstream_changed: ${{ steps.check.outputs.upstream_changed }}
    upstream_digest: ${{ steps.check.outputs.upstream_digest }}
    rpm_changed: ${{ steps.rpm-check.outputs.rpm_changed }}
  steps:
    - name: Check upstream digest
      id: check
      # ... existing digest check ...

    - name: Check RPM versions
      id: rpm-check
      if: github.event_name == 'schedule' && steps.check.outputs.upstream_changed == 'false'
      run: |
        # Read system packages from manifest
        PACKAGES=$(jq -r '.packages[].name' manifests/system-packages.json | tr '\n' ' ')

        # Get versions from current published image label
        CURRENT_VERSIONS=$(jq -r '.Labels["org.wycats.bootc.rpm.versions"] // ""' /tmp/current.json)

        # Query repos for latest versions (using published image as container)
        LATEST_VERSIONS=$(docker run --rm "ghcr.io/${GITHUB_REPOSITORY}:latest" \
          dnf repoquery --latest-limit=1 --qf '%{name}:%{version}-%{release}' $PACKAGES 2>/dev/null | sort | tr '\n' ',')

        if [[ "$CURRENT_VERSIONS" != "$LATEST_VERSIONS" ]]; then
          echo "rpm_changed=true" >> "$GITHUB_OUTPUT"
        else
          echo "rpm_changed=false" >> "$GITHUB_OUTPUT"
        fi
```

### Modified Build Condition

```yaml
build:
  if: |
    github.event_name == 'push' ||
    github.event_name == 'pull_request_target' ||
    github.event_name == 'workflow_dispatch' ||
    needs.check-upstream.outputs.upstream_changed == 'true' ||
    needs.check-upstream.outputs.rpm_changed == 'true'
```

The `nightly` job's `force_rebuild` logic is replaced by the `rpm_changed` output from `check-upstream`.

### Simplified Schedule

The two crons can be merged into one hourly cron, since the RPM check now runs on every scheduled invocation:

```yaml
schedule:
  # Hourly check for upstream digest changes AND RPM updates
  - cron: "0 * * * *"
```

### Modified Build Step (Embed Versions)

During the build, after packages are installed, query and embed RPM versions as an OCI label. This happens in the `docker/build-push-action` step via the `labels` input, using a version string collected from the built image.

## Cost Analysis

- **Current**: 24 hourly digest checks (cheap, ~30s each) + 1 nightly forced build (~15min)
- **Proposed**: 24 hourly checks with RPM query (~2min each due to container pull) + builds only when something changed
- **Net**: Slightly more CI minutes on checks, but fewer unnecessary builds. On weeks with no RPM updates, saves the nightly build entirely.

## Migration

1. Modify build step to embed RPM version label
2. Add RPM version check to `check-upstream` job
3. Update build condition to use `rpm_changed`
4. Remove `nightly` job and its `force_rebuild` output
5. Remove nightly cron (`0 3 * * *`) — hourly cron now handles everything

## Open Questions

1. **Container pull cost**: Pulling the full image just to run `dnf repoquery` adds ~1-2 minutes per hourly check. Could we use a lighter approach (e.g., parsing repo metadata XML directly)?
2. **Race condition**: Between the RPM check and the actual build, a newer version could land. This is acceptable — the next hourly check will catch it.
3. **Label format**: Should the version label be JSON or a simple key:value comma-separated string?
