---
description: "Enforces Truth: Adherence to Physics/Axioms."
---

# Axiomatic Coherence Check

**Focus**: Adherence to Physics (Truth).
**Grimoire Mapping**: `ax` (Axioms), `native` (Project Native), `novel` (Novelty Search).

## The Prime Directive

**Don't just work; Belong.**
Code that functions but violates the system's axioms is "Hypocritical Code". It must be refactored or removed.

## Protocol

1.  **Extract Axioms**

    - Read `docs/agent-context/axioms.workflow.toml`.
    - Read `docs/agent-context/axioms.system.toml`.
    - Read `docs/design/axioms.design.toml`.
    - Internalize the "Why" of the system.

2.  **Audit Implementation**

    - Scan `src/` for "Median Code" (generic patterns, boilerplate, "laundry list" features).
    - Identify code that violates specific axioms (e.g., "No Hidden State", "Phased Execution", "Context is King").

3.  **Generative Refactor**
    - **Derive, Don't List**: Rewrite code to _derive_ behavior from first principles rather than hardcoding edge cases.
    - **Native Alignment**: Refactor generic implementations to use Project Native patterns (e.g., using the `Context` object instead of passing 10 arguments).

## Output

- Identification of "Hypocritical Code".
- Refactoring plan to align code with the scoped axiom TOML files.
