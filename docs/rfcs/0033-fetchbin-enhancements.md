# RFC 0033: Fetchbin Enhancements

- Feature Name: `fetchbin_enhancements`
- Start Date: 2026-01-28
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

This RFC proposes future enhancements for `fetchbin` focused on multi-binary packages, idempotent installs, self-healing verification, npm integrity checks, manifest verification metadata, lifecycle script support, runtime management improvements, and platform-specific GitHub asset matching.

## Motivation

The current implementation is functional but lacks ergonomics and hardening features that reduce re-downloads, improve integrity verification, and simplify multi-binary installs. These enhancements improve security, reliability, and user experience while keeping `fetchbin` lightweight.

## Guide-level Explanation

### New user-facing capabilities

- Install all binaries from multi-binary packages:

  ```bash
  fetchbin install npm:@biomejs/biome --all-bins
  ```

- Interactive selection when multiple binaries exist and no `--bin` provided:

  ```bash
  fetchbin install npm:@biomejs/biome
  ```

- Verify and self-heal during listing:

  ```bash
  fetchbin list --verify
  fetchbin repair
  ```

- Enforce verification:

  ```bash
  fetchbin install npm:turbo --require-verified
  ```

- Allow npm lifecycle scripts explicitly:

  ```bash
  fetchbin install npm:some-package --allow-scripts
  ```

- Improved GitHub asset matching for platform variants and universal macOS binaries.

## Reference-level Explanation

### 1. Multi-Binary Package Improvements

**Motivation**: Multi-binary packages are common and currently require the user to pass `--bin` or fail. Supporting `--all-bins` and interactive selection reduces friction.

**Implementation plan**:

- Add `--all-bins` flag to install all binaries from a package.
- Add interactive selection if multiple binaries are detected and `--bin` is not supplied.
- Persist each binary as a separate `InstalledBinary` entry referencing the same package/version.

**Source-specific details**:

- **npm**: iterate over all entries in the `bin` field.
- **cargo**: query crate metadata for binary names (use crates.io `/versions` or `crate` metadata if available).
- **GitHub**: parse release notes or apply naming conventions to infer multiple binaries; fall back to `--bin` or `--all-bins` only when unambiguous.

**API/CLI changes**:

- `fetchbin install <spec> --all-bins`
- `fetchbin install <spec>` prompts if multiple binaries available

**Test cases**:

- `install_all_bins_npm`
- `install_select_prompt_npm`
- `install_all_bins_cargo`
- `install_all_bins_github`

**Estimated complexity**: M

### 2. Idempotent Installs

**Motivation**: Re-downloading on repeated installs is slow and wastes bandwidth.

**Implementation plan**:

- Check `manifest.json` for an existing entry with matching name + version.
- If the binary exists on disk and its checksum matches the manifest, skip download.
- If checksum mismatches or file missing, reinstall.

**API/CLI changes**:

- Default behavior becomes idempotent; no new flags required.
- Optional `--force` to re-download (if not already present).

**Test cases**:

- `install_skips_when_present_and_verified`
- `install_reinstalls_when_missing`
- `install_reinstalls_on_checksum_mismatch`

**Estimated complexity**: M

### 3. Self-Healing

**Motivation**: Users should see integrity issues and get automatic repair options.

**Implementation plan**:

- Add a `verify()` method to check binary existence and checksum.
- `list --verify` triggers verification for each entry and reports health status.
- Add a `repair` subcommand to reinstall corrupted or missing binaries.

**API/CLI changes**:

- `fetchbin list --verify`
- `fetchbin repair`

**Test cases**:

- `list_verify_marks_missing`
- `list_verify_marks_checksum_mismatch`
- `repair_reinstalls_broken`

**Estimated complexity**: M

### 4. npm Integrity Verification

**Motivation**: npm provides SRI hashes via `dist.integrity`; verifying tarballs improves security.

**Implementation plan**:

- Parse the `dist.integrity` field (format `sha512-<base64>`).
- Compute the SHA-512 hash of the downloaded tarball.
- Compare before extraction or installation.

**API/CLI changes**:

- None (enabled by default when available).

**Test cases**:

- `npm_integrity_verification_success`
- `npm_integrity_verification_failure`

**Estimated complexity**: S

### 5. Manifest Enhancements

**Motivation**: Users need visibility into verification status and methods used.

**Implementation plan**:

- Add `verified: bool` and `verification_method: "sha256" | "sha512" | "none"` to manifest entries.
- Add `--require-verified` to refuse installation if verification is unavailable.

**API/CLI changes**:

- `fetchbin install <spec> --require-verified`

**Test cases**:

- `require_verified_rejects_unverified`
- `manifest_writes_verification_metadata`

**Estimated complexity**: S

### 6. Lifecycle Script Support

**Motivation**: Some npm packages require `postinstall` to build their binaries.

**Implementation plan**:

- Add `--allow-scripts` to opt into lifecycle scripts.
- Run scripts in a constrained environment:
  - Controlled PATH
  - No network by default (if feasible)
  - Temporary working directory

**API/CLI changes**:

- `fetchbin install <spec> --allow-scripts`

**Test cases**:

- `allow_scripts_runs_postinstall`
- `disallow_scripts_fails_with_guidance`

**Estimated complexity**: M

### 7. Runtime Management Improvements

**Motivation**: Keep Node LTS more current and avoid stale runtime info.

**Implementation plan**:

- On `bkt status`, check for LTS updates (cache for 24 hours).
- Cache LTS version in runtime.json with a timestamp.
- Prune unused runtimes after operations (already partially implemented).

**API/CLI changes**:

- No new `fetchbin` flags; behavior is automatic.

**Test cases**:

- `runtime_lts_cache_respected`
- `runtime_lts_refreshes_after_ttl`

**Estimated complexity**: S

### 8. Platform-Specific Asset Patterns

**Motivation**: GitHub releases vary widely in naming; better heuristics reduce manual `--asset` usage.

**Implementation plan**:

- Expand asset matching with additional patterns per OS/arch.
- Support macOS universal binaries (`universal`, `universal2`).
- Improve Windows `.exe` detection and ZIP selection.

**API/CLI changes**:

- None (internal heuristics only).

**Test cases**:

- `asset_match_macos_universal`
- `asset_match_windows_exe`
- `asset_match_prefers_arch_specific`

**Estimated complexity**: S

## Drawbacks

- Adds complexity to the CLI and manifest schema.
- Increases the test surface area and maintenance burden.
- Lifecycle scripts raise security risks and need careful sandboxing.

## Rationale and Alternatives

- **Why in `fetchbin`**: Centralizes binary acquisition logic instead of delegating to external tools.
- **Alternative**: Use system package managers or toolchain managers (e.g., asdf, mise), but they add global state and complexity.
- **Alternative**: Keep `fetchbin` minimal and leave advanced workflows to users, at the cost of usability and safety.

## Prior art

- `pipx` for isolated CLI installs
- `cargo-binstall` for prebuilt Rust binaries
- `npm` integrity checking via SRI
- `asdf`/`mise` for runtime version caching

## Unresolved questions

- How should GitHub multi-binary detection work when release notes are unstructured?
- Should `--require-verified` be default-on in a future major release?
- What sandboxing constraints are feasible for lifecycle scripts across platforms?
