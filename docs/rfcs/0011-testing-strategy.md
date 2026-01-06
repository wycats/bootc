# RFC-0011: Testing Strategy and Dependency Injection

- **Status**: Draft
- **Created**: 2026-01-05

## Summary

This RFC defines a comprehensive testing strategy for the `bkt` tool, centered around dependency injection via the `PrBackend` trait. The goal is to enable tests that exercise real code paths without requiring production environment dependencies (git, gh, network, repo.json).

## Motivation

### The Problem

Tests that work locally often fail in CI due to environment differences:

- CI environment lacks `gh auth` (GitHub CLI not authenticated)
- CI lacks `git config user.email` and `user.name`
- CI lacks `/usr/share/bootc/repo.json` (only exists in built bootc image)

This leads to two bad patterns:

1. **Skipping tests in CI**: Tests are marked with `#[ignore]` or special flags
2. **Test mode flags**: Code sprinkled with `if env::var("TEST_MODE")...`

Both approaches mean tests don't verify production code paths.

### The Goal

Tests should:

- ✅ Exercise the full command flow including PR decision logic
- ✅ Run in any environment (CI, local, container)
- ✅ Verify exact manifest content that would be committed
- ✅ Assert on branch names, PR titles, commit messages
- ❌ NOT require GitHub authentication
- ❌ NOT need git repository configuration
- ❌ NOT rely on special environment variables

## Design: PrBackend Trait

### Core Abstraction

The key insight is that PR creation is a **side effect** that can be abstracted behind a trait:

```rust
/// Trait for PR creation operations - enables testing without git/gh
pub trait PrBackend: Send + Sync {
    /// Create a PR with the given manifest changes
    fn create_pr(
        &self,
        change: &PrChange,
        manifest_content: &str,
        skip_preflight: bool,
    ) -> Result<()>;
}
```

### Production Implementation: GitHubBackend

The real implementation delegates to existing code:

```rust
/// Production backend that uses real git/gh commands
pub struct GitHubBackend;

impl PrBackend for GitHubBackend {
    fn create_pr(
        &self,
        change: &PrChange,
        manifest_content: &str,
        skip_preflight: bool,
    ) -> Result<()> {
        run_pr_workflow(change, manifest_content, skip_preflight)
    }
}
```

This means:

- Zero changes to the actual PR workflow logic
- Preflight checks run as before
- Git operations remain unchanged
- Real production path is preserved

### Test Implementation: MockPrBackend

The test backend records calls for later assertions:

```rust
/// Test backend that records PR creation calls
#[derive(Default)]
pub struct MockPrBackend {
    calls: Arc<Mutex<Vec<PrCall>>>,
}

#[derive(Debug, Clone)]
pub struct PrCall {
    pub change: PrChange,
    pub manifest_content: String,
    pub skip_preflight: bool,
}

impl MockPrBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all recorded PR creation calls
    pub fn calls(&self) -> Vec<PrCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Assert that no PRs were created
    pub fn assert_no_pr(&self) {
        assert!(self.calls().is_empty(), "Expected no PRs, got {}", self.calls().len());
    }
}

impl PrBackend for MockPrBackend {
    fn create_pr(
        &self,
        change: &PrChange,
        manifest_content: &str,
        skip_preflight: bool,
    ) -> Result<()> {
        self.calls.lock().unwrap().push(PrCall {
            change: change.clone(),
            manifest_content: manifest_content.to_string(),
            skip_preflight,
        });
        Ok(())
    }
}
```

Tests can now assert on:

- Branch names: `call.change.branch_name() == "bkt/flatpak/add-boxes"`
- PR titles: `call.change.pr_title() == "Add flatpak: org.gnome.Boxes"`
- Manifest content: `call.manifest_content.contains("org.gnome.Boxes")`
- Number of PRs created: `mock.calls().len() == 2`

### Integration with ExecutionPlan

The `ExecutionPlan` holds a reference to the backend:

```rust
pub struct ExecutionPlan {
    pub context: ExecutionContext,
    pub pr_mode: PrMode,
    pub dry_run: bool,
    pub skip_preflight: bool,
    pr_backend: Arc<dyn PrBackend>,
}

impl ExecutionPlan {
    /// Create an execution plan from CLI arguments (uses real backend)
    pub fn from_cli(cli: &Cli) -> Self {
        Self {
            context: resolve_context(cli.context),
            pr_mode: /* ... */,
            dry_run: cli.dry_run,
            skip_preflight: cli.skip_preflight,
            pr_backend: Arc::new(GitHubBackend),
        }
    }

    /// Create a PR using the configured backend
    pub fn maybe_create_pr(&self, ...) -> Result<()> {
        if self.should_create_pr() {
            let change = PrChange { ... };
            self.pr_backend.create_pr(&change, manifest_content, self.skip_preflight)?;
        }
        Ok(())
    }
}
```

## Test Categories

### 1. Unit Tests

Test individual functions with direct mocks:

```rust
#[test]
fn test_sanitize_for_ref() {
    assert_eq!(sanitize_for_ref("org.gnome.Boxes"), "org-gnome-Boxes");
}
```

### 2. Integration Tests with MockPrBackend

Test full command flow with dependency injection:

```rust
#[test]
fn test_flatpak_add_creates_pr_with_correct_content() {
    let mock = Arc::new(MockPrBackend::new());
    let plan = ExecutionPlanBuilder::new()
        .pr_mode(PrMode::PrOnly)
        .pr_backend(mock.clone())
        .build();

    run(args, &plan).unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].manifest_content.contains("org.gnome.Boxes"));
}
```

### 3. Preflight Check Tests

Test preflight checks independently as unit tests.

### 4. E2E Tests (Optional/Manual)

For comprehensive verification, manual E2E tests can be run in properly configured environments.

## Benefits

1. **Fast feedback**: Tests run in <100ms without network/git
2. **Reliable CI**: No flaky failures due to auth/network issues
3. **Real code paths**: Tests exercise production logic, not test-mode branches
4. **Comprehensive assertions**: Can verify exact PR content
5. **Clean separation**: Side effects isolated behind trait boundary

## Implementation Plan

### Phase 1: Define Trait and Production Backend

1. Add `PrBackend` trait to `pr.rs`
2. Implement `GitHubBackend` that wraps `run_pr_workflow`

### Phase 2: Thread Backend Through ExecutionPlan

1. Add `pr_backend` field to `ExecutionPlan`
2. Update `ExecutionPlan::from_cli` to use `GitHubBackend`
3. Update `maybe_create_pr` to use trait
4. Update `ExecutionPlanBuilder` to accept backend

### Phase 3: Create MockPrBackend

1. Implement `MockPrBackend` with call recording
2. Add assertion helpers

### Phase 4: Update Existing Tests

1. Convert integration tests to use `MockPrBackend`
2. Remove test-mode environment variable checks
3. Remove `--skip-preflight` flags from tests

## Alternatives Considered

### Alternative 1: Environment Variable Test Mode

Rejected because tests don't verify production code and pollute production with test concerns.

### Alternative 2: Use `--local` Flag in Tests

Rejected because it doesn't test the PR decision logic at all.

## Success Criteria

- [ ] All CI tests pass without GitHub auth
- [ ] All CI tests pass without git configuration
- [ ] Tests verify exact PR content
- [ ] No test-mode conditionals in production code

## Related RFCs

- **RFC-0008** (Command Infrastructure): Establishes plan/execute pattern
