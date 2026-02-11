# RFC 0019: Environment Abstraction

One-line summary: Provide a testable environment and runtime context layer for bkt command execution.

## Motivation

bkt needs to reason about where it is running (host vs toolbox vs container) and select an execution context without scattering platform checks across commands. It also needs environment-sensitive logic that remains testable in Rust 2024, where mutating global environment variables is unsafe. A dedicated abstraction keeps environment inspection consistent and allows unit tests to control filesystem and environment inputs.

## Design

The environment layer has two parts: a testable `Environment` trait for reading process and filesystem state, and runtime detection utilities that decide execution context.

- `Environment` provides read-only access to environment variables, user directories, working directory, and basic filesystem queries (exists, is_file, is_dir, read_to_string).
- Convenience helpers `expand_home` and `collapse_home` convert `~/` paths in a consistent way.
- `RealEnvironment` implements the trait using `std::env` and `directories` to provide production behavior.
- `MockEnvironment` provides an in-memory environment for tests, including variables, files, directories, and a current working directory.

Runtime detection uses small, explicit signals:

- `RuntimeEnvironment` is `Host`, `Toolbox`, or generic `Container`.
- Detection checks `/run/.toolboxenv`, `/run/.containerenv`, and `CONTAINER_ID`. Toolbox markers take precedence.
- `BKT_FORCE_HOST=1` overrides detection for tests.
- `is_in_toolbox` is the canonical helper for toolbox/container detection.

Execution selection is explicit and independent of runtime detection:

- `ExecutionContext` is `host`, `dev`, or `image` (manifest-only).
- `resolve_context` chooses an explicit context when provided, otherwise auto-detects `dev` when running in toolbox and `host` otherwise.
- `CommandDomain` validation enforces which command families are valid in each context.

## Implementation Notes

- The abstraction and detection live in [bkt/src/context.rs](bkt/src/context.rs).
- `REAL_ENV` is a global instance used by production functions so callers do not need to thread an environment object.
- `MockEnvironment` stores `vars`, `files`, and `dirs` and exposes builder-style helpers for tests (`with_var`, `with_file`, `with_dir`, `with_cwd`).
- `Environment::expand_home` and `Environment::collapse_home` are implemented as default trait methods so both real and mock environments share logic.
- `detect_environment_with_env` and `resolve_context_with_env` are the testable variants that accept a trait object, keeping tests deterministic.

## Known Gaps

None.
