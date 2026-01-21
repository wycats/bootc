# Quality Backlog — Codebase Improvements

This document captures the current quality-focused improvement backlog. It is the next area of focus, and once completed will be archived and folded back into CURRENT.md.

---

## How to Use This Document

- **Goal**: Improve reliability, maintainability, security, and developer experience across the codebase.
- **Structure**: Each item lists **category**, **issue**, **location**, **suggested fix**, and **priority**.
- **Status**: Track progress by adding checkboxes or moving completed items into a future archive.

---

## High Priority

### 1) Replace panics in production paths

- **Category**: Reliability
- **Issue**: Many `unwrap()` / `expect()` calls exist in production code paths, risking panics.
- **Location**: Multiple modules in bkt/src (command execution, PR workflows, manifest I/O)
- **Suggested fix**:
  - Replace `unwrap()` / `expect()` in production code with `?` and `.context(...)`
  - Keep `unwrap()` in tests only
  - Add lints or CI check to fail on `unwrap()` outside tests
- **Priority**: High

### 2) Add integration tests for core commands

- **Category**: Testing
- **Issue**: Integration tests are limited; many command modules lack coverage.
- **Location**: bkt/tests
- **Suggested fix**:
  - Add integration tests for: extension, gsetting, admin, upstream, changelog, local, containerfile, drift
  - Add end-to-end workflow tests: capture → apply → verify
  - Add CLI parsing tests for error/edge cases
- **Priority**: High

### 3) Add coverage reporting in CI

- **Category**: CI/CD
- **Issue**: No coverage visibility for tests.
- **Location**: .github/workflows
- **Suggested fix**:
  - Add coverage step using `cargo tarpaulin`
  - Upload coverage to Codecov (or similar)
- **Priority**: High

### 4) Validate all user inputs before invoking system commands

- **Category**: Security
- **Issue**: Not all user inputs are validated before use in shell/system commands.
- **Location**: Commands handling extension IDs, package names, flatpak IDs, file paths
- **Suggested fix**:
  - Centralize validation helpers
  - Validate identifiers with allowlisted patterns
  - Normalize and reject unsafe characters
- **Priority**: High

---

## Medium Priority

### 5) Reduce unnecessary cloning in hot paths

- **Category**: Performance
- **Issue**: Excessive `.clone()` usage, especially in manifest handling and pipeline planning.
- **Location**: bkt/src/manifest/\*, bkt/src/pipeline.rs
- **Suggested fix**:
  - Use borrowed references where ownership isn’t required
  - Use `Cow<'_, T>` to avoid cloning unless needed
  - Profile for hot allocations and refactor accordingly
- **Priority**: Medium

### 6) Improve API documentation coverage

- **Category**: Documentation
- **Issue**: Many public items lack doc comments.
- **Location**: Public APIs across bkt/src
- **Suggested fix**:
  - Add module-level docs for major subsystems
  - Document public structs, enums, and functions
  - Add examples for complex APIs
- **Priority**: Medium

### 7) Split binary and library crates

- **Category**: Code Organization
- **Issue**: All logic currently lives in the binary crate, making testing and reuse harder.
- **Location**: bkt/src
- **Suggested fix**:
  - Add `lib.rs` for shared functionality
  - Keep `main.rs` as a thin CLI entry point
  - Move core logic into reusable modules
- **Priority**: Medium

### 8) Use stable Rust edition

- **Category**: Tooling Compatibility
- **Issue**: `edition = "2024"` may reduce compatibility with stable toolchains.
- **Location**: bkt/Cargo.toml
- **Suggested fix**:
  - Move to `edition = "2021"` until 2024 edition is fully stable
- **Priority**: Medium

### 9) Migrate critical scripts to Rust

- **Category**: Maintainability
- **Issue**: Drift detection and bootstrap scripts live in Bash/Python.
- **Location**: scripts/check-drift, scripts/bootc-bootstrap
- **Suggested fix**:
  - Port drift detection to Rust (bkt drift)
  - Keep wrapper scripts but move core logic into Rust
- **Priority**: Medium

### 10) Expand property-based tests

- **Category**: Testing
- **Issue**: Property tests are minimal and do not cover core invariants.
- **Location**: bkt/tests/properties.rs
- **Suggested fix**:
  - Add invariants for manifest merge idempotence/commutativity
  - Test PR workflow state machine
  - Add schema validation round-trips
- **Priority**: Medium

---

## Low Priority

### 11) Add security scanning to CI

- **Category**: Security / CI
- **Issue**: No automated dependency/security scanning.
- **Location**: .github/workflows
- **Suggested fix**:
  - Add `cargo audit`
  - Add GitHub dependency review action
- **Priority**: Low

### 12) Optimize lefthook performance

- **Category**: Developer Experience
- **Issue**: Pre-commit hooks run full clippy, slowing down local dev.
- **Location**: lefthook.yml
- **Suggested fix**:
  - Move heavy checks to pre-push
  - Add lightweight pre-commit checks only
- **Priority**: Low

### 13) Standardize error message format

- **Category**: Consistency
- **Issue**: Error messages vary in phrasing/capitalization.
- **Location**: Multiple modules
- **Suggested fix**:
  - Adopt a consistent style guide
  - Normalize error prefixes and context usage
- **Priority**: Low

### 14) Add schema examples

- **Category**: Documentation
- **Issue**: JSON schemas lack example payloads.
- **Location**: schemas/\*.json
- **Suggested fix**:
  - Add `examples` sections for each schema
  - Reference real manifest fragments
- **Priority**: Low

### 15) Extract magic numbers

- **Category**: Code Quality
- **Issue**: Numeric constants without context exist in code.
- **Location**: Multiple modules
- **Suggested fix**:
  - Create named constants with comments
- **Priority**: Low

### 16) Profile and parallelize manifest loading

- **Category**: Performance
- **Issue**: Manifests loaded sequentially even when independent.
- **Location**: Command flows that load multiple manifests
- **Suggested fix**:
  - Profile to confirm bottleneck
  - Consider parallel loading with Rayon
- **Priority**: Low

---

## Notes & Next Steps

- Start with **High Priority** items to reduce risk and improve reliability.
- Use this document as the authoritative quality backlog until it is archived and folded into CURRENT.md.
