---
description: "Verifies Logic: Consistency of the Graph."
---

# Internal Coherence Check

**Focus**: Logical Consistency (Logic).
**Grimoire Mapping**: `inv` (Inversion), `fuzz` (Fuzz Logic).

## The Prime Directive

**Verify the Graph.**
The system is a graph of connected nodes (Goals, Decisions, Docs, Code). Broken edges are failures.

## Protocol

1.  **Cross-Reference Check**
    - **Plan vs Decisions**: Does `docs/agent-context/plan.toml` contain goals that implement "Rejected" decisions in `decisions.toml`?
    - **Plan vs Ideas**: Are "Active" goals linked to "Approved" ideas?

2.  **Link Rot Detection**
    - Scan `docs/` for broken file links.
    - Verify that referenced symbols (classes, functions) in documentation still exist in the codebase.

3.  **Schema Validation**
    - Ensure all TOML files (`plan.toml`, `decisions.toml`, etc.) conform to their Zod schemas (if available) or structural expectations.

## Output

- List of logical contradictions.
- List of broken links.
- Fixes for the internal graph.
